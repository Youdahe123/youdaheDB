use std::fs::File;

use std::io::{self, BufWriter, Write};

use std::path::{Path, PathBuf};

use crate::memtable::{Lookup, MemTable};

pub struct SSTable {
    index: Vec<(String, u64)>,
    path: PathBuf,
}

const TOMBSTONE: u32 = u32::MAX; // the WAL hardcodes u32::MAX inline but you're going to ref it in 3 places in this file so we are going to name it once

impl SSTable {

    pub fn flush_from_memtable(memtable: &MemTable, path: &Path) -> io::Result<SSTable> {
        todo!()
    }

    // load the index into memory the entries stay on disk
    pub fn open(path: &Path) -> io::Result<SSTable> {
        todo!()
    }

    // binary search the index and get the value, one seek one read
    pub fn get(&self, key: &str) -> io::Result<Lookup> {
        todo!()
    }

    // used for compaction later
    pub fn iter_entries(&self) {
        todo!()
    }
}
