use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

// single operation recorded in the wal
pub struct WalEntry {
    pub key: String,
    pub value: Option<String>, // None = deleted
}

// write ahead log
pub struct Wal {
    writer: BufWriter<File>,
    path: String,
}

impl Wal {
    pub fn open(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Wal {
            writer: BufWriter::new(file),
            path: path.to_string(),
        })
    }
}
