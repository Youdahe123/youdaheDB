use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::memtable::{Lookup, MemTable};
use crate::merge::{memtable_source, EntryIter, MergeIter};
use crate::sstable::{sync_parent_dir, SSTable, TMP_SUFFIX};
use crate::wal::Wal;

const WAL_NAME: &str = "data.wal";

// zero padded so a lexicographic sort matches a numeric one
fn sst_name(id: u64) -> String {
    format!("sst-{id:06}.sst")
}

// the counter has to survive a restart, so it is recovered from the highest
// filename on disk rather than started from zero - otherwise the first flush
// after reopening overwrites an existing table
fn sst_id(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let digits = name.strip_prefix("sst-")?.strip_suffix(".sst")?;
    digits.parse().ok()
}

fn is_tmp_table(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("sst-") && name.ends_with(TMP_SUFFIX))
}

pub struct LsmTree {
    dir: PathBuf,
    wal: Wal,
    memtable: MemTable,
    sstables: Vec<SSTable>, // newest first
    next_id: u64,
    // Keep the OS lock until after the WAL and tables have been dropped.
    // Never unlink LOCK: a new inode would allow two independent owners.
    _directory_lock: fs::File,
}

impl LsmTree {
    pub fn open(dir: &str) -> io::Result<LsmTree> {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir)?;

        // Acquire ownership before reading recovery state or opening the WAL.
        // The OS releases the lock on close or process death; the file remains.
        let directory_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("LOCK"))?;
        directory_lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("database directory is already open: {}", dir.display()),
            ),
            fs::TryLockError::Error(error) => error,
        })?;

        // a .tmp is a table whose flush never finished; its data is still in
        // the wal, so the file is garbage
        let mut ids = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if is_tmp_table(&path) {
                fs::remove_file(&path)?;
            } else if let Some(id) = sst_id(&path) {
                ids.push(id);
            }
        }
        ids.sort_unstable_by(|a, b| b.cmp(a)); // newest first

        let next_id = ids.first().map_or(0, |highest| highest + 1);

        let mut sstables = Vec::with_capacity(ids.len());
        for id in ids {
            sstables.push(SSTable::open(&dir.join(sst_name(id)))?);
        }

        let wal_path = dir.join(WAL_NAME);
        let wal_path = wal_path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "data directory is not valid utf-8",
            )
        })?;

        // rebuild whatever was in the memtable when the process died
        let mut memtable = MemTable::new();
        for entry in Wal::replay(wal_path)? {
            match entry.value {
                Some(value) => memtable.put(entry.key, value),
                None => memtable.delete(&entry.key),
            }
        }

        Ok(LsmTree {
            wal: Wal::open(wal_path)?,
            dir,
            memtable,
            sstables,
            next_id,
            _directory_lock: directory_lock,
        })
    }

    // the wal write returns only after sync_all, so reaching the memtable line
    // means the record is already on disk. A crash between them would lose a
    // write that was already acknowledged.
    pub fn put(&mut self, key: &str, value: &str) -> io::Result<()> {
        self.wal.put(key, value)?;
        self.memtable.put(key.to_string(), value.to_string());
        self.flush_if_full()
    }

    // tombstones are writes too, so this can fill the memtable as well
    pub fn delete(&mut self, key: &str) -> io::Result<()> {
        self.wal.delete(key)?;
        self.memtable.delete(key);
        self.flush_if_full()
    }

    // Deleted has to stop the search. Falling through to an older layer that
    // still holds the key brings the deleted value back.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        match self.memtable.get(key) {
            Lookup::Found(value) => return Ok(Some(value)),
            Lookup::Deleted => return Ok(None),
            Lookup::NotFound => {}
        }

        for sstable in &self.sstables {
            match sstable.get(key)? {
                Lookup::Found(value) => return Ok(Some(value)),
                Lookup::Deleted => return Ok(None),
                Lookup::NotFound => {}
            }
        }

        Ok(None)
    }

    /// Sources in version order; callers select bounds before hiding tombstones.
    pub(crate) fn scan_merge(&self) -> io::Result<MergeIter<'_>> {
        let mut sources: Vec<EntryIter> = Vec::with_capacity(self.sstables.len() + 1);
        sources.push(memtable_source(&self.memtable));
        for sstable in &self.sstables {
            sources.push(Box::new(sstable.iter_entries()?));
        }
        Ok(MergeIter::new(sources))
    }

    /// Writes the memtable out as a new SSTable, then clears the log.
    pub fn flush(&mut self) -> io::Result<()> {
        if self.memtable.is_empty() {
            return Ok(());
        }

        let path = self.dir.join(sst_name(self.next_id));
        let sstable = SSTable::flush_from_memtable(&self.memtable, &path)?;
        self.next_id += 1;

        // the log can only be dropped once the table is durable - clearing it
        // first leaves a window where a crash loses data held in neither place
        self.wal.clear()?;
        self.memtable.clear();

        self.sstables.insert(0, sstable);
        Ok(())
    }

    /// Merges every SSTable into one, keeping only the newest version of each
    /// key. The memtable is left alone.
    ///
    /// Merging everything is the simplest correct policy: with no older run
    /// left behind, every tombstone can be dropped. Leveled compaction (#10)
    /// rewrites far less data per pass but leaves more runs for reads to search,
    /// and has to check the levels below before it can drop a tombstone.
    pub fn compact(&mut self) -> io::Result<()> {
        if self.sstables.len() < 2 {
            return Ok(());
        }

        let path = self.dir.join(sst_name(self.next_id));

        let mut sources: Vec<EntryIter> = Vec::with_capacity(self.sstables.len());
        for sstable in &self.sstables {
            sources.push(Box::new(sstable.iter_entries()?));
        }
        let mut merged = MergeIter::new(sources)
            .live()
            .map(|entry| entry.map(|(k, v)| (k, Some(v))))
            .peekable();

        // everything was deleted: an empty table would only cost an extra file
        let compacted = match merged.peek() {
            Some(_) => {
                let sstable = SSTable::write_sorted(merged, &path)?;
                self.next_id += 1;
                Some(sstable)
            }
            None => None,
        };

        // The merged table has the highest id, so while the old tables still
        // exist it shadows them - except for keys it dropped as deleted. A crash
        // partway through leaves some old tables behind, and one of them could
        // still hold a value whose tombstone the merge discarded.
        //
        // Deleting oldest-first closes that hole. A tombstone always lives in a
        // newer table than the value it hides, so the value's table goes first,
        // and whatever survives a crash still reads the same. Each unlink is
        // made durable before the next so the disk can't reorder them.
        // #8's MANIFEST replaces this with a single atomic switch.
        let old = std::mem::take(&mut self.sstables);
        self.sstables.extend(compacted);

        for i in (0..old.len()).rev() {
            // on failure, keep serving the tables still on disk rather than
            // reading as if they were gone until the next reopen
            if let Err(e) = fs::remove_file(old[i].path()) {
                self.sstables.extend(old.into_iter().take(i + 1));
                return Err(e);
            }
            if let Err(e) = sync_parent_dir(old[i].path()) {
                self.sstables.extend(old.into_iter().take(i));
                return Err(e);
            }
        }

        Ok(())
    }

    pub fn sstable_count(&self) -> usize {
        self.sstables.len()
    }

    fn flush_if_full(&mut self) -> io::Result<()> {
        if self.memtable.is_full() {
            self.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // no tempfile crate, so directories are named by pid plus a counter
    fn temp_dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "youdahedb-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn open(dir: &Path) -> LsmTree {
        LsmTree::open(dir.to_str().unwrap()).expect("open failed")
    }

    #[test]
    fn write_then_read() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();

        assert_eq!(db.get("k").unwrap(), Some("v".to_string()));
        assert_eq!(db.get("missing").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_finds_a_key_after_it_was_flushed() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();
        db.flush().unwrap();

        assert!(db.memtable.is_empty(), "flush must empty the memtable");
        assert_eq!(db.sstable_count(), 1);
        // the whole point of #3: this read has to reach disk
        assert_eq!(db.get("k").unwrap(), Some("v".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn newest_version_wins_across_layers() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "oldest").unwrap();
        db.flush().unwrap();
        db.put("k", "middle").unwrap();
        db.flush().unwrap();
        db.put("k", "newest").unwrap();

        assert_eq!(db.sstable_count(), 2);
        assert_eq!(db.get("k").unwrap(), Some("newest".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // the easiest bug in the read path: continuing past a tombstone into an
    // older sstable that still holds the value
    #[test]
    fn delete_in_memtable_hides_a_flushed_value() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();
        db.flush().unwrap();
        db.delete("k").unwrap();

        assert_eq!(db.get("k").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flushed_tombstone_still_hides_an_older_value() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();
        db.flush().unwrap();
        db.delete("k").unwrap();
        db.flush().unwrap();

        assert_eq!(db.get("k").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn put_after_delete_and_flush_revives_the_key() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v1").unwrap();
        db.delete("k").unwrap();
        db.flush().unwrap();
        db.put("k", "v2").unwrap();

        assert_eq!(db.get("k").unwrap(), Some("v2".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // dropping without a clean shutdown, then reopening - the wal is the only
    // thing standing between an unflushed write and losing it
    #[test]
    fn unflushed_writes_survive_a_reopen() {
        let dir = temp_dir();
        {
            let mut db = open(&dir);
            db.put("a", "1").unwrap();
            db.put("b", "2").unwrap();
            db.delete("a").unwrap();
        }

        let db = open(&dir);

        assert_eq!(db.get("a").unwrap(), None);
        assert_eq!(db.get("b").unwrap(), Some("2".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flushed_data_survives_a_reopen() {
        let dir = temp_dir();
        {
            let mut db = open(&dir);
            db.put("a", "1").unwrap();
            db.flush().unwrap();
            db.put("b", "2").unwrap();
        }

        let db = open(&dir);

        assert_eq!(db.sstable_count(), 1);
        assert_eq!(db.get("a").unwrap(), Some("1".to_string()));
        assert_eq!(db.get("b").unwrap(), Some("2".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // if reopening loaded them oldest-first, this returns the stale value
    #[test]
    fn reopen_loads_sstables_newest_first() {
        let dir = temp_dir();
        {
            let mut db = open(&dir);
            db.put("k", "old").unwrap();
            db.flush().unwrap();
            db.put("k", "new").unwrap();
            db.flush().unwrap();
        }

        let db = open(&dir);

        assert_eq!(db.sstable_count(), 2);
        assert_eq!(db.get("k").unwrap(), Some("new".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // a fresh counter would overwrite sst-000000 and lose the first table
    #[test]
    fn flush_after_reopen_does_not_overwrite_an_existing_table() {
        let dir = temp_dir();
        {
            let mut db = open(&dir);
            db.put("a", "1").unwrap();
            db.flush().unwrap();
        }

        let mut db = open(&dir);
        db.put("b", "2").unwrap();
        db.flush().unwrap();

        assert_eq!(db.sstable_count(), 2);
        assert_eq!(db.get("a").unwrap(), Some("1".to_string()));
        assert_eq!(db.get("b").unwrap(), Some("2".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // a crash mid-flush leaves a half-written table behind. It must not stop
    // the directory from opening, and the write it held must come back from
    // the wal instead
    #[test]
    fn a_half_written_table_from_a_crashed_flush_is_ignored_on_open() {
        let dir = temp_dir();
        {
            let mut db = open(&dir);
            db.put("a", "1").unwrap();
            db.flush().unwrap();
            db.put("b", "2").unwrap();
        }

        // what a kill between File::create and the footer write leaves behind
        let partial = dir.join(format!("{}{TMP_SUFFIX}", sst_name(1)));
        fs::write(&partial, [7u8; 5]).unwrap();

        let mut db = open(&dir);

        assert!(!partial.exists(), "leftover .tmp should be deleted");
        assert_eq!(db.sstable_count(), 1);
        assert_eq!(db.get("a").unwrap(), Some("1".to_string()));
        assert_eq!(db.get("b").unwrap(), Some("2".to_string()));

        db.flush().unwrap();
        assert_eq!(db.sstable_count(), 2);
        assert_eq!(db.get("b").unwrap(), Some("2".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flush_leaves_no_tmp_file_behind() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();
        db.flush().unwrap();

        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.contains(&sst_name(0)));
        assert!(!names.iter().any(|n| n.ends_with(TMP_SUFFIX)), "{names:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flushing_an_empty_memtable_writes_nothing() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.flush().unwrap();

        assert_eq!(db.sstable_count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_merges_layers_in_order_and_hides_tombstones() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("b", "old").unwrap();
        db.put("d", "4").unwrap();
        db.flush().unwrap();
        db.put("a", "1").unwrap();
        db.put("b", "new").unwrap();
        db.delete("d").unwrap();

        let out: Vec<_> = db
            .scan_merge()
            .unwrap()
            .live()
            .map(|e| e.unwrap())
            .collect();

        assert_eq!(
            out,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "new".to_string()),
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn memtable_filling_up_flushes_on_its_own() {
        let dir = temp_dir();
        let mut db = open(&dir);
        db.memtable = MemTable::with_capacity(16);

        for i in 0..10 {
            db.put(&format!("key{i}"), "value").unwrap();
        }

        assert!(
            db.sstable_count() > 0,
            "writes should have triggered a flush"
        );
        assert_eq!(db.get("key0").unwrap(), Some("value".to_string()));
        assert_eq!(db.get("key9").unwrap(), Some("value".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // every live key and value, read through the same path users see
    fn snapshot(db: &LsmTree) -> Vec<(String, String)> {
        db.scan_merge()
            .unwrap()
            .live()
            .map(|e| e.unwrap())
            .collect()
    }

    fn sst_files(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| sst_id(p).is_some())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn compaction_keeps_only_the_newest_version() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "old").unwrap();
        db.flush().unwrap();
        db.put("k", "new").unwrap();
        db.flush().unwrap();

        db.compact().unwrap();

        assert_eq!(db.sstable_count(), 1);
        assert_eq!(db.get("k").unwrap(), Some("new".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    // the tombstone is dropped, so if the older value made it into the merged
    // table there would be nothing left to hide it
    #[test]
    fn compaction_does_not_resurrect_a_deleted_key() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("gone", "v").unwrap();
        db.put("kept", "v").unwrap();
        db.flush().unwrap();
        db.delete("gone").unwrap();
        db.flush().unwrap();

        db.compact().unwrap();
        assert_eq!(db.get("gone").unwrap(), None);
        drop(db);

        let db = open(&dir);
        assert_eq!(db.get("gone").unwrap(), None);
        assert_eq!(db.get("kept").unwrap(), Some("v".to_string()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compaction_drops_superseded_versions_and_tombstones_from_disk() {
        let dir = temp_dir();
        let mut db = open(&dir);

        for round in 0..3 {
            db.put("a", &format!("a{round}")).unwrap();
            db.put("b", &format!("b{round}")).unwrap();
            db.flush().unwrap();
        }
        db.delete("b").unwrap();
        db.flush().unwrap();

        db.compact().unwrap();

        let entries: Vec<_> = db.sstables[0]
            .iter_entries()
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(entries, vec![("a".to_string(), Some("a2".to_string()))]);
        fs::remove_dir_all(&dir).unwrap();
    }

    // the invariant that matters: compaction is invisible to readers
    #[test]
    fn reads_are_identical_before_and_after_compaction() {
        let dir = temp_dir();
        let mut db = open(&dir);

        for round in 0..5 {
            for i in 0..20 {
                let key = format!("key{i:02}");
                match (i + round) % 4 {
                    0 => db.delete(&key).unwrap(),
                    _ => db.put(&key, &format!("r{round}")).unwrap(),
                }
            }
            db.flush().unwrap();
        }
        // unflushed writes sit above the compacted table and must still win
        db.put("key00", "memtable").unwrap();
        db.delete("key01").unwrap();

        let before = snapshot(&db);
        let gets_before: Vec<_> = (0..20)
            .map(|i| db.get(&format!("key{i:02}")).unwrap())
            .collect();

        db.compact().unwrap();
        assert_eq!(db.sstable_count(), 1);
        assert_eq!(snapshot(&db), before);
        let gets_after: Vec<_> = (0..20)
            .map(|i| db.get(&format!("key{i:02}")).unwrap())
            .collect();
        assert_eq!(gets_after, gets_before);
        drop(db);

        let db = open(&dir);
        assert_eq!(snapshot(&db), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compacting_only_deleted_keys_leaves_no_tables() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("k", "v").unwrap();
        db.flush().unwrap();
        db.delete("k").unwrap();
        db.flush().unwrap();

        db.compact().unwrap();

        assert_eq!(db.sstable_count(), 0);
        assert!(sst_files(&dir).is_empty());
        assert_eq!(db.get("k").unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compaction_after_reopen_does_not_overwrite_an_existing_table() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("a", "1").unwrap();
        db.flush().unwrap();
        db.put("b", "2").unwrap();
        db.flush().unwrap();
        db.compact().unwrap();
        db.put("c", "3").unwrap();
        db.flush().unwrap();
        drop(db);

        let mut db = open(&dir);
        db.compact().unwrap();
        assert_eq!(snapshot(&db).len(), 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    // Simulates a crash at every point in the cleanup: the merged table is
    // installed, and some newest-first suffix of the old tables is still on
    // disk because deletion runs oldest-first. Every one of those states has to
    // read the same as the finished compaction.
    #[test]
    fn a_crash_partway_through_cleanup_does_not_resurrect_anything() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.put("dead", "v").unwrap();
        db.put("live", "old").unwrap();
        db.flush().unwrap();
        db.put("live", "new").unwrap();
        db.flush().unwrap();
        db.delete("dead").unwrap();
        db.flush().unwrap();

        let old_files = sst_files(&dir); // oldest first
        let saved: Vec<Vec<u8>> = old_files.iter().map(|p| fs::read(p).unwrap()).collect();
        let expected = snapshot(&db);

        db.compact().unwrap();
        drop(db);

        for survivors in 0..=old_files.len() {
            let first_survivor = old_files.len() - survivors;
            for (path, bytes) in old_files.iter().zip(&saved).skip(first_survivor) {
                fs::write(path, bytes).unwrap();
            }

            let db = open(&dir);
            assert_eq!(
                snapshot(&db),
                expected,
                "{survivors} old table(s) left behind"
            );
            assert_eq!(db.get("dead").unwrap(), None);
            drop(db);

            for path in &old_files {
                let _ = fs::remove_file(path);
            }
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compacting_one_table_or_none_is_a_no_op() {
        let dir = temp_dir();
        let mut db = open(&dir);

        db.compact().unwrap();
        assert_eq!(db.sstable_count(), 0);

        db.put("k", "v").unwrap();
        db.flush().unwrap();
        let files = sst_files(&dir);
        db.compact().unwrap();
        assert_eq!(sst_files(&dir), files);
        fs::remove_dir_all(&dir).unwrap();
    }
}
