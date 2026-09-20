use std::io;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::lsm::LsmTree;

/// A shared database: concurrent readers, one exclusive writer.
///
/// Clones share the same tree and lock. A second open of the same directory
/// fails with `io::ErrorKind::WouldBlock`, including from another process.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<RwLock<LsmTree>>,
}

/// A consistent read view that streams entries without collecting the dataset.
///
/// Holds a read lock until dropped, including after an iterator is exhausted.
/// Keep its scope short: writers wait for this guard. Do not call other Engine
/// methods while holding it (even a recursive read can deadlock behind a queued
/// writer on some platforms). Use `iter()` to read this view instead.
#[must_use = "a scan guard holds a read lock until dropped"]
pub struct Scan<'a> {
    tree: RwLockReadGuard<'a, LsmTree>,
    selection: Selection,
}

enum Selection {
    All,
    Range {
        start: Option<String>,
        end: Option<String>,
    },
    Prefix(String),
}

impl Scan<'_> {
    /// Streams sorted live entries, retaining only the merge iterator's state.
    /// I/O errors can occur both when opening the sources and while iterating.
    pub fn iter(&self) -> io::Result<impl Iterator<Item = io::Result<(String, String)>> + '_> {
        let merge = self.tree.scan_merge()?;
        Ok(match &self.selection {
            Selection::All => merge,
            Selection::Range { start, end } => merge.with_range(start.as_deref(), end.as_deref()),
            Selection::Prefix(prefix) => merge.with_prefix(prefix),
        }
        .live())
    }
}

impl Engine {
    /// Exclusively opens a directory and replays its write-ahead log.
    /// The directory becomes available again when the last clone is dropped.
    pub fn open(dir: &str) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(LsmTree::open(dir)?)),
        })
    }

    fn read(&self) -> io::Result<RwLockReadGuard<'_, LsmTree>> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let result = self
            .inner
            .read()
            .map_err(|_| io::Error::other("engine lock poisoned"));
        #[cfg(test)]
        benchmark::record_wait(start.elapsed());
        result
    }

    fn write(&self) -> io::Result<RwLockWriteGuard<'_, LsmTree>> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let result = self
            .inner
            .write()
            .map_err(|_| io::Error::other("engine lock poisoned"));
        #[cfg(test)]
        benchmark::record_wait(start.elapsed());
        result
    }

    /// Reads the newest value, or `None` for a missing or deleted key.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        self.read()?.get(key)
    }

    /// Writes durably before returning. Holds the write lock through any flush.
    /// An I/O error does not guarantee that the write was rolled back.
    pub fn put(&self, key: &str, value: &str) -> io::Result<()> {
        self.write()?.put(key, value)
    }

    /// Records a durable deletion, hiding any older value.
    pub fn delete(&self, key: &str) -> io::Result<()> {
        self.write()?.delete(key)
    }

    /// Locks a consistent view of all live entries for streaming.
    ///
    /// Drop the returned [`Scan`] to allow writers to proceed. See its docs for
    /// lock lifetime and reentrancy restrictions.
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> {
    /// let db = youdaheDB::Engine::open("data")?;
    /// {
    ///     let scan = db.scan()?;
    ///     for entry in scan.iter()? {
    ///         let (key, value) = entry?;
    ///         println!("{key} = {value}");
    ///     }
    /// } // releases the read lock before the next write
    /// db.put("key", "value")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn scan(&self) -> io::Result<Scan<'_>> {
        Ok(Scan {
            tree: self.read()?,
            selection: Selection::All,
        })
    }

    /// Locks a view of live keys in the half-open interval `[start, end)`.
    ///
    /// `None` leaves that endpoint unbounded. Equal or reversed endpoints yield
    /// no rows. Keys use Rust string ordering without Unicode normalization.
    /// Bounds are copied, so their inputs need not outlive the returned guard.
    /// The guard holds its read lock until dropped, even for an empty range.
    /// Iteration streams from the beginning of each source rather than seeking;
    /// errors below the lower bound can therefore also be returned.
    ///
    /// ```no_run
    /// use youdaheDB::Engine;
    ///
    /// fn main() -> std::io::Result<()> {
    ///     let db = Engine::open("./data")?;
    ///     db.put("user:1", "youdahe")?;
    ///     {
    ///         let scan = db.scan_range(Some("user:"), Some("user;"))?;
    ///         for entry in scan.iter()? {
    ///             let (key, value) = entry?;
    ///             println!("{key} = {value}");
    ///         }
    ///     } // releases the read lock before the next write
    ///     db.delete("user:1")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn scan_range(&self, start: Option<&str>, end: Option<&str>) -> io::Result<Scan<'_>> {
        Ok(Scan {
            tree: self.read()?,
            selection: Selection::Range {
                start: start.map(str::to_owned),
                end: end.map(str::to_owned),
            },
        })
    }

    /// Locks a view of live keys starting with `prefix`; an empty prefix matches all.
    ///
    /// Matching uses `str::starts_with`, without Unicode normalization. The prefix
    /// is copied. Lock lifetime, streaming, and errors follow [`Self::scan_range`].
    pub fn scan_prefix(&self, prefix: &str) -> io::Result<Scan<'_>> {
        Ok(Scan {
            tree: self.read()?,
            selection: Selection::Prefix(prefix.to_owned()),
        })
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

#[cfg(test)]
mod benchmark;
