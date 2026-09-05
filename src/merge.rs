use std::io;
use std::iter::Peekable;

use crate::memtable::MemTable;

// one record as it flows between layers. None is a tombstone, not an absence -
// the distinction has to survive the merge or deletes get undone.
pub type Entry = (String, Option<String>);

// every source is normalised to this. SSTables read from disk so they can fail;
// the memtable can't, but it wears the Result anyway so both fit in one Vec.
pub type EntryIter<'a> = Box<dyn Iterator<Item = io::Result<Entry>> + 'a>;

// where a scan stops. Prefix is its own variant rather than a computed upper
// bound because incrementing the last byte of a String can produce invalid
// UTF-8, and "still starts with p" needs no such trick.
enum Bound {
    None,
    Excl(String),
    Prefix(String),
}

/// Walks several sorted sources as if they were one sorted stream.
///
/// Sources are given newest-first. Every key is yielded exactly once, carrying
/// the newest version of that key. Tombstones ARE yielded - compaction needs to
/// see them to know when they can be dropped, so filtering is the caller's job.
/// `live()` does that filtering for read paths.
pub struct MergeIter<'a> {
    sources: Vec<Peekable<EntryIter<'a>>>,
    start: Option<String>,
    end: Bound,
}

impl<'a> MergeIter<'a> {
    /// `sources` in precedence order: newest first, oldest last.
    pub fn new(sources: Vec<EntryIter<'a>>) -> Self {
        MergeIter {
            sources: sources.into_iter().map(|s| s.peekable()).collect(),
            start: None,
            end: Bound::None,
        }
    }

    /// Half-open `[start, end)`. Either side may be omitted.
    pub fn with_range(mut self, start: Option<&str>, end: Option<&str>) -> Self {
        self.start = start.map(str::to_string);
        self.end = match end {
            Some(e) => Bound::Excl(e.to_string()),
            None => Bound::None,
        };
        self
    }

    /// Every key beginning with `prefix`.
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.start = Some(prefix.to_string());
        self.end = Bound::Prefix(prefix.to_string());
        self
    }

    /// Drops tombstones. This is what a user-facing `scan` wants; compaction
    /// wants the unfiltered iterator.
    pub fn live(self) -> impl Iterator<Item = io::Result<(String, String)>> + 'a {
        self.filter_map(|res| match res {
            Ok((_, None)) => None,
            Ok((k, Some(v))) => Some(Ok((k, v))),
            Err(e) => Some(Err(e)),
        })
    }

    // past the end of the requested range?
    fn beyond_end(&self, key: &str) -> bool {
        match &self.end {
            Bound::None => false,
            Bound::Excl(e) => key >= e.as_str(),
            // Sorted order means the first key at-or-past the prefix that stops
            // matching ends the scan. Keys BELOW the prefix aren't past the end -
            // they're before the start, and the start bound skips them.
            Bound::Prefix(p) => key >= p.as_str() && !key.starts_with(p.as_str()),
        }
    }

    // a source whose head is an Err has to surface it rather than be skipped
    // over, or a read failure turns into a silently short scan
    fn take_pending_error(&mut self) -> Option<io::Result<Entry>> {
        for s in self.sources.iter_mut() {
            if matches!(s.peek(), Some(Err(_))) {
                return s.next();
            }
        }
        None
    }

    // smallest key across all heads, cloned so the borrow on the sources ends
    // before we start consuming from them
    fn smallest_key(&mut self) -> Option<String> {
        let mut min: Option<String> = None;
        for s in self.sources.iter_mut() {
            if let Some(Ok((k, _))) = s.peek() {
                if min.as_deref().map_or(true, |m| k.as_str() < m) {
                    min = Some(k.clone());
                }
            }
        }
        min
    }
}

impl<'a> Iterator for MergeIter<'a> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(err) = self.take_pending_error() {
                return Some(err);
            }

            let key = self.smallest_key()?;

            if self.beyond_end(&key) {
                return None;
            }

            // Consume this key from EVERY source that holds it, not just the
            // winner. Leaving the losers parked on it makes the next call
            // return the same key again with a stale value.
            let mut winner: Option<Entry> = None;
            for s in self.sources.iter_mut() {
                let holds = matches!(s.peek(), Some(Ok((k, _))) if *k == key);
                if !holds {
                    continue;
                }
                match s.next() {
                    // sources are newest-first, so the first one to match wins
                    Some(Ok(entry)) => {
                        if winner.is_none() {
                            winner = Some(entry);
                        }
                    }
                    Some(Err(e)) => return Some(Err(e)),
                    None => unreachable!("peek said there was an entry"),
                }
            }

            // below the requested start: consumed from every layer, yielded to
            // nobody. Skipping forward here is O(n); seeking each source with
            // its own index is the faster version and belongs with #6.
            if let Some(start) = &self.start {
                if key < *start {
                    continue;
                }
            }

            return winner.map(Ok);
        }
    }
}

/// Makes the memtable look like every other source: owned strings, wrapped in
/// Ok. The clone is the price of one uniform item type across layers.
pub fn memtable_source(memtable: &MemTable) -> EntryIter<'_> {
    Box::new(memtable.iter().map(|(k, v)| Ok((k.clone(), v.clone()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    // build a source from literals. None is a tombstone.
    fn src(entries: &[(&str, Option<&str>)]) -> EntryIter<'static> {
        let owned: Vec<io::Result<Entry>> = entries
            .iter()
            .map(|(k, v)| Ok((k.to_string(), v.map(str::to_string))))
            .collect();
        Box::new(owned.into_iter())
    }

    fn failing() -> EntryIter<'static> {
        Box::new(std::iter::once(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated sstable",
        ))))
    }

    fn collect(it: MergeIter) -> Vec<(String, Option<String>)> {
        it.map(|r| r.expect("no io error expected")).collect()
    }

    fn keys(it: MergeIter) -> Vec<String> {
        collect(it).into_iter().map(|(k, _)| k).collect()
    }

    #[test]
    fn merges_disjoint_sources_in_order() {
        let m = MergeIter::new(vec![
            src(&[("b", Some("2"))]),
            src(&[("a", Some("1")), ("c", Some("3"))]),
        ]);
        assert_eq!(keys(m), vec!["a", "b", "c"]);
    }

    // the bug this whole design exists to avoid: a key present in three layers
    // must come out once, not three times
    #[test]
    fn duplicate_key_is_yielded_exactly_once() {
        let m = MergeIter::new(vec![
            src(&[("dup", Some("newest"))]),
            src(&[("dup", Some("middle"))]),
            src(&[("dup", Some("oldest"))]),
        ]);
        assert_eq!(collect(m), vec![("dup".to_string(), Some("newest".to_string()))]);
    }

    #[test]
    fn newest_source_wins_a_tie() {
        let m = MergeIter::new(vec![
            src(&[("a", Some("new")), ("b", Some("new"))]),
            src(&[("a", Some("old")), ("c", Some("old"))]),
        ]);
        assert_eq!(
            collect(m),
            vec![
                ("a".to_string(), Some("new".to_string())),
                ("b".to_string(), Some("new".to_string())),
                ("c".to_string(), Some("old".to_string())),
            ]
        );
    }

    // interleaving is where a merge that forgets to advance the losers shows up
    // as duplicates rather than as a wrong value
    #[test]
    fn interleaved_overlapping_sources() {
        let m = MergeIter::new(vec![
            src(&[("a", Some("1")), ("c", Some("3")), ("e", Some("5"))]),
            src(&[("b", Some("2")), ("c", Some("x")), ("d", Some("4"))]),
            src(&[("a", Some("x")), ("f", Some("6"))]),
        ]);
        assert_eq!(keys(m), vec!["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn tombstone_is_yielded_by_the_raw_iterator() {
        let m = MergeIter::new(vec![src(&[("gone", None)]), src(&[("gone", Some("v"))])]);
        assert_eq!(collect(m), vec![("gone".to_string(), None)]);
    }

    #[test]
    fn live_drops_tombstones() {
        let m = MergeIter::new(vec![
            src(&[("gone", None), ("keep", Some("v"))]),
            src(&[("gone", Some("old"))]),
        ]);
        let out: Vec<_> = m.live().map(|r| r.unwrap()).collect();
        assert_eq!(out, vec![("keep".to_string(), "v".to_string())]);
    }

    // a delete in a MIDDLE layer still shadows a live value underneath it.
    // If tie-breaking were backwards this returns "resurrected".
    #[test]
    fn tombstone_in_middle_layer_shadows_older_value() {
        let m = MergeIter::new(vec![
            src(&[("other", Some("x"))]),
            src(&[("k", None)]),
            src(&[("k", Some("resurrected"))]),
        ]);
        let out: Vec<_> = m.live().map(|r| r.unwrap()).collect();
        assert_eq!(out, vec![("other".to_string(), "x".to_string())]);
    }

    // put-after-delete: the newer layer revives the key
    #[test]
    fn newer_value_overrides_older_tombstone() {
        let m = MergeIter::new(vec![src(&[("k", Some("v2"))]), src(&[("k", None)])]);
        let out: Vec<_> = m.live().map(|r| r.unwrap()).collect();
        assert_eq!(out, vec![("k".to_string(), "v2".to_string())]);
    }

    #[test]
    fn empty_sources_are_ignored() {
        let m = MergeIter::new(vec![src(&[]), src(&[("a", Some("1"))]), src(&[])]);
        assert_eq!(keys(m), vec!["a"]);
    }

    #[test]
    fn no_sources_yields_nothing() {
        assert_eq!(keys(MergeIter::new(vec![])), Vec::<String>::new());
    }

    #[test]
    fn range_start_is_inclusive_and_end_is_exclusive() {
        let m = MergeIter::new(vec![src(&[
            ("a", Some("1")),
            ("b", Some("2")),
            ("c", Some("3")),
            ("d", Some("4")),
        ])])
        .with_range(Some("b"), Some("d"));
        assert_eq!(keys(m), vec!["b", "c"]);
    }

    #[test]
    fn range_bounds_apply_across_layers() {
        let m = MergeIter::new(vec![
            src(&[("a", Some("1")), ("c", Some("3"))]),
            src(&[("b", Some("2")), ("d", Some("4"))]),
        ])
        .with_range(Some("b"), Some("d"));
        assert_eq!(keys(m), vec!["b", "c"]);
    }

    #[test]
    fn prefix_scan_stops_at_the_first_non_match() {
        let m = MergeIter::new(vec![src(&[
            ("user:1", Some("a")),
            ("user:2", Some("b")),
            ("usor", Some("c")),
            ("zzz", Some("d")),
        ])])
        .with_prefix("user:");
        assert_eq!(keys(m), vec!["user:1", "user:2"]);
    }

    #[test]
    fn prefix_skips_keys_before_the_range() {
        let m = MergeIter::new(vec![src(&[
            ("aaa", Some("x")),
            ("user:1", Some("a")),
        ])])
        .with_prefix("user:");
        assert_eq!(keys(m), vec!["user:1"]);
    }

    // a source that fails mid-stream must surface the error, not silently end
    // the scan early - a short scan looks like missing data
    #[test]
    fn read_error_is_surfaced() {
        let mut m = MergeIter::new(vec![src(&[("a", Some("1"))]), failing()]);
        let first = m.next().expect("expected an item");
        assert!(first.is_err(), "the error at a head must come out first");
    }

    #[test]
    fn memtable_source_matches_the_common_shape() {
        let mut mt = MemTable::new();
        mt.put("b".to_string(), "2".to_string());
        mt.put("a".to_string(), "1".to_string());
        mt.delete("c");

        let m = MergeIter::new(vec![memtable_source(&mt)]);
        assert_eq!(
            collect(m),
            vec![
                ("a".to_string(), Some("1".to_string())),
                ("b".to_string(), Some("2".to_string())),
                ("c".to_string(), None),
            ]
        );
    }

    // the shape #3 will actually use: memtable over SSTable-like layers
    #[test]
    fn memtable_shadows_older_layers() {
        let mut mt = MemTable::new();
        mt.put("k".to_string(), "from-memtable".to_string());

        let m = MergeIter::new(vec![
            memtable_source(&mt),
            src(&[("j", Some("older")), ("k", Some("from-sstable"))]),
        ]);
        assert_eq!(
            collect(m),
            vec![
                ("j".to_string(), Some("older".to_string())),
                ("k".to_string(), Some("from-memtable".to_string())),
            ]
        );
    }
}
