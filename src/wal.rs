use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
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

    // tombstone marker for deletes
    pub fn delete(&mut self, key: &str) -> io::Result<()> {
        let key_bytes = key.as_bytes();

        self.writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(key_bytes)?;
        self.writer.write_all(&u32::MAX.to_le_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    // read the entire wal from disk and return all entries
    pub fn replay(path: &str) -> io::Result<Vec<WalEntry>> {
        if !Path::new(path).exists() {
            return Ok(vec![]);
        }

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        loop {
            // read key length
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
            let key_len = u32::from_le_bytes(len_buf) as usize;

            // read key
            let mut key_buf = vec![0u8; key_len];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            // read value length
            reader.read_exact(&mut len_buf)?;
            let val_len = u32::from_le_bytes(len_buf);

            if val_len == u32::MAX {
                // tombstone
                entries.push(WalEntry { key, value: None });
            } else {
                let mut val_buf = vec![0u8; val_len as usize];
                reader.read_exact(&mut val_buf)?;
                let value = String::from_utf8(val_buf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                entries.push(WalEntry {
                    key,
                    value: Some(value),
                });
            }
        }

        Ok(entries)
    }

    // clear the wal after flushing to sstable
    pub fn clear(&mut self) -> io::Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.writer = BufWriter::new(file);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_replay() {
        let path = "test_wal_put.wal";

        let mut wal = Wal::open(path).unwrap();
        wal.put("key1", "value1").unwrap();
        wal.put("key2", "value2").unwrap();
        drop(wal);

        let entries = Wal::replay(path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "key1");
        assert_eq!(entries[0].value, Some("value1".to_string()));
        assert_eq!(entries[1].key, "key2");
        assert_eq!(entries[1].value, Some("value2".to_string()));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_delete_and_replay() {
        let path = "test_wal_del.wal";

        let mut wal = Wal::open(path).unwrap();
        wal.put("key1", "value1").unwrap();
        wal.delete("key1").unwrap();
        drop(wal);

        let entries = Wal::replay(path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].value, None);

        std::fs::remove_file(path).unwrap();
    }
}
