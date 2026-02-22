use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub struct WalEntry {
    pub key: String,
    pub value: Option<String>,
}

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

    // format: [key_len: 4 bytes][key][val_len: 4 bytes][value]
    pub fn put(&mut self, key: &str, value: &str) -> io::Result<()> {
        let key_bytes = key.as_bytes();
        let val_bytes = value.as_bytes();

        self.writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(key_bytes)?;
        self.writer.write_all(&(val_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(val_bytes)?;
        self.writer.flush()?;

        Ok(())
    }

    // delete uses u32::MAX as tombstone marker for the value length
    pub fn delete(&mut self, key: &str) -> io::Result<()> {
        let key_bytes = key.as_bytes();

        self.writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(key_bytes)?;
        self.writer.write_all(&u32::MAX.to_le_bytes())?;
        self.writer.flush()?;

        Ok(())
    }
}
