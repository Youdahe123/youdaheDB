use std::io;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::lsm::LsmTree;

/// A shared database: concurrent readers, one exclusive writer.
///
/// Clones share the same tree and lock. Open each directory only once; this
/// lock does not coordinate separate opens or separate processes.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<RwLock<LsmTree>>,
}

impl Engine {
    /// Opens existing data and replays its write-ahead log.
    pub fn open(dir: &str) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(LsmTree::open(dir)?)),
        })
    }

    fn read(&self) -> io::Result<RwLockReadGuard<'_, LsmTree>> {
        self.inner
            .read()
            .map_err(|_| io::Error::other("engine lock poisoned"))
    }

    fn write(&self) -> io::Result<RwLockWriteGuard<'_, LsmTree>> {
        self.inner
            .write()
            .map_err(|_| io::Error::other("engine lock poisoned"))
    }

    /// Reads the newest value, or `None` for a missing or deleted key.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        self.read()?.get(key)
    }

    /// Writes durably before returning. Holds the write lock through any flush.
    pub fn put(&self, key: &str, value: &str) -> io::Result<()> {
        self.write()?.put(key, value)
    }

    /// Records a durable deletion, hiding any older value.
    pub fn delete(&self, key: &str) -> io::Result<()> {
        self.write()?.delete(key)
    }

    /// Collects a consistent, sorted view of all live entries.
    ///
    /// Writers wait until collection finishes. The returned values own their
    /// memory and hold no lock; memory usage grows with the result size.
    pub fn scan(&self) -> io::Result<Vec<(String, String)>> {
        let tree = self.read()?;
        let entries = tree.scan()?.collect();
        entries
    }

    /// Flushes the current memtable while excluding other operations.
    pub fn flush(&self) -> io::Result<()> {
        self.write()?.flush()
    }

    /// Returns the current number of immutable disk tables.
    pub fn sstable_count(&self) -> io::Result<usize> {
        Ok(self.read()?.sstable_count())
    }
}

#[cfg(test)]
mod tests;
