use std::collections::BTreeMap;

// result of looking a key up in ONE layer. The distinction between Deleted and
// NotFound is what keeps the read path correct once sstables exist: Deleted
// means stop searching, NotFound means an older layer might still have it.
// Collapsing them into a plain Option would resurrect deleted keys.
pub enum Lookup {
    Found(String),
    Deleted,
    NotFound,
}

// sorted in memory key value store, sits on top of the wal
// keys are always sorted because of btreemap which makes flushing to disk efficient
pub struct MemTable {
    entries: BTreeMap<String, Option<String>>,
    size_bytes: usize,
    max_size: usize,
}

impl MemTable {
    const MAXSIZE: usize = 4 * 1024 * 1024;
    pub fn new() -> Self {
        Self::with_capacity(Self::MAXSIZE)
    }
    pub fn with_capacity(n : usize) -> Self {
        MemTable {
            entries: BTreeMap::new(),
            size_bytes: 0,
            max_size: n,
        }
    }

    pub fn put(&mut self, key: String, value: String) {
        self.insert(key, Some(value));
    }

    // store tombstone instead of removing
    pub fn delete(&mut self, key: &str) {
        self.insert(key.to_string(), None);
    }

    // overwriting a key replaces its bytes rather than adding to them, so the
    // old entry's size has to come back off the running total — otherwise
    // size_bytes drifts upward and triggers flushes far too early
    fn insert(&mut self, key: String, value: Option<String>) {
        let added = Self::entry_size(&key, &value);
        let removed = self
            .entries
            .get(&key)
            .map_or(0, |old| Self::entry_size(&key, old));
        self.entries.insert(key, value);
        self.size_bytes = self.size_bytes + added - removed;
    }

    fn entry_size(key: &str, value: &Option<String>) -> usize {
        key.len() + value.as_ref().map_or(0, |v| v.len())
    }

    pub fn get(&self, key: &str) -> Lookup {
        match self.entries.get(key) {
            Some(Some(value)) => Lookup::Found(value.clone()),
            Some(None) => Lookup::Deleted,
            None => Lookup::NotFound,
        }
    }

    pub fn size(&self) -> usize {
        self.size_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Option<String>)> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.size_bytes = 0;
    }
    pub fn is_full(&self) -> bool{
        self.size_bytes >= self.max_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // pulls the string out of a Found, failing loudly on anything else
    fn expect_found(lookup: Lookup) -> String {
        match lookup {
            Lookup::Found(value) => value,
            Lookup::Deleted => panic!("expected Found, got Deleted"),
            Lookup::NotFound => panic!("expected Found, got NotFound"),
        }
    }

    #[test]
    fn test_put_and_get() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "value1".to_string());

        assert_eq!(expect_found(mt.get("key1")), "value1");
        assert!(matches!(mt.get("key2"), Lookup::NotFound));
    }

    #[test]
    fn test_delete_creates_tombstone() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "value1".to_string());
        mt.delete("key1");

        assert!(matches!(mt.get("key1"), Lookup::Deleted));
    }

    // deleting a key this memtable never saw still has to record a tombstone —
    // that key may live in an sstable below, and this is the marker that
    // shadows it. A delete that searched first and bailed would fail here.
    #[test]
    fn test_delete_of_unseen_key_still_tombstones() {
        let mut mt = MemTable::new();
        mt.delete("never-written");

        assert!(matches!(mt.get("never-written"), Lookup::Deleted));
    }

    // a tombstone is not permanent: writing the key again brings it back
    #[test]
    fn test_put_after_delete_revives_key() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "v1".to_string());
        mt.delete("key1");
        mt.put("key1".to_string(), "v2".to_string());

        assert_eq!(expect_found(mt.get("key1")), "v2");
    }

    #[test]
    fn test_overwrite() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "v1".to_string());
        mt.put("key1".to_string(), "v2".to_string());

        assert_eq!(expect_found(mt.get("key1")), "v2");
    }

    // overwriting must not double count — size tracks current contents, not
    // everything ever written, or the flush trigger fires much too early
    #[test]
    fn test_overwrite_does_not_inflate_size() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "v1".to_string());
        let after_first = mt.size();
        mt.put("key1".to_string(), "v2".to_string());

        assert_eq!(mt.size(), after_first);
    }

    #[test]
    fn test_sorted_iteration() {
        let mut mt = MemTable::new();
        mt.put("c".to_string(), "3".to_string());
        mt.put("a".to_string(), "1".to_string());
        mt.put("b".to_string(), "2".to_string());

        let keys: Vec<&String> = mt.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_new_memtable_is_not_full() {
        let mt = MemTable::new();

        assert!(!mt.is_full());
        assert_eq!(mt.len(), 0);
        assert_eq!(mt.size(), 0);
    }

    // with_capacity is what makes this testable - against the 4MB default this
    // test would have to write four million bytes to trip the threshold
    #[test]
    fn test_is_full_once_threshold_crossed() {
        let mut mt = MemTable::with_capacity(10);
        assert!(!mt.is_full());

        // 4 + 8 = 12 bytes, past the 10 byte limit
        mt.put("key1".to_string(), "longvalue".to_string());

        assert!(mt.is_full());
    }

    // the threshold is >=, not >, so landing exactly on it counts as full
    #[test]
    fn test_is_full_at_exactly_the_threshold() {
        let mut mt = MemTable::with_capacity(6);

        // 3 + 3 = 6 bytes exactly
        mt.put("abc".to_string(), "xyz".to_string());

        assert_eq!(mt.size(), 6);
        assert!(mt.is_full());
    }

    // the one that matters: overwriting a hot key must not inflate the size.
    // If insert() stopped subtracting the displaced entry, 100 writes to one
    // key would trip a threshold that a single entry is nowhere near - and
    // flushes would fire constantly on a workload with hot keys.
    #[test]
    fn test_overwriting_one_key_never_trips_is_full() {
        let mut mt = MemTable::with_capacity(50);

        for i in 0..100 {
            mt.put("hot".to_string(), format!("value{i}"));
        }

        assert_eq!(mt.len(), 1, "overwrites must not add entries");
        assert!(!mt.is_full(), "size must track current contents, not total writes");
    }

    // tombstones are writes too - a delete-only workload still has to flush
    #[test]
    fn test_tombstones_count_toward_the_threshold() {
        let mut mt = MemTable::with_capacity(20);

        for i in 0..10 {
            mt.delete(&format!("key{i}"));
        }

        assert!(mt.is_full());
    }

    // clear() runs after a flush, so the memtable has to come back empty and
    // ready to accept writes again
    #[test]
    fn test_clear_resets_full_state() {
        let mut mt = MemTable::with_capacity(10);
        mt.put("key1".to_string(), "longvalue".to_string());
        assert!(mt.is_full());

        mt.clear();

        assert!(!mt.is_full());
        assert_eq!(mt.len(), 0);
        assert_eq!(mt.size(), 0);
    }
}
