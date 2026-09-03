use std::fs::File;

use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};

use std::path::{Path, PathBuf};

use crate::memtable::{Lookup, MemTable};

pub struct SSTable {
    index: Vec<(String, u64)>,
    path: PathBuf,
    index_offset: u64,
}

pub struct SSTableIter {
    reader: BufReader<File>,
    remaining: u64,
}

const TOMBSTONE: u32 = u32::MAX; // the WAL hardcodes u32::MAX inline but you're going to ref it in 3 places in this file so we are going to name it once

impl SSTable {

    pub fn flush_from_memtable(memtable: &MemTable, path: &Path) -> io::Result<SSTable> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let mut index: Vec<(String, u64)> = Vec::with_capacity(memtable.len());
        let mut offset: u64 = 0;

        for (key, value) in memtable.iter() {
            index.push((key.clone(), offset));

            writer.write_all(&(key.len() as u32).to_le_bytes())?;
            writer.write_all(key.as_bytes())?;

            let value_len = match value {
                Some(v) => {
                    writer.write_all(&(v.len() as u32).to_le_bytes())?;
                    writer.write_all(v.as_bytes())?;
                    4 + v.len()
                }
                None => {
                    writer.write_all(&TOMBSTONE.to_le_bytes())?;
                    4
                }
            };

            offset += (4 + key.len() + value_len) as u64;
        }

        let index_offset = offset;

        for (key, entry_offset) in &index {
            writer.write_all(&(key.len() as u32).to_le_bytes())?;
            writer.write_all(key.as_bytes())?;
            writer.write_all(&entry_offset.to_le_bytes())?;
        }

        writer.write_all(&index_offset.to_le_bytes())?;

        writer.flush()?;
        writer.get_ref().sync_all()?;

        Ok(SSTable {
            index,
            path: path.to_path_buf(),
            index_offset,
        })
    }

    // load the index into memory the entries stay on disk
    pub fn open(path: &Path) -> io::Result<SSTable> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();

        if len < 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sstable is too short to hold a footer",
            ));
        }

        file.seek(SeekFrom::Start(len - 8))?;
        let mut footer = [0u8; 8];
        file.read_exact(&mut footer)?;
        let index_offset = u64::from_le_bytes(footer);

        if index_offset > len - 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "footer points past the end of the index block",
            ));
        }

        let index_len = len - 8 - index_offset;
        file.seek(SeekFrom::Start(index_offset))?;
        let mut reader = BufReader::new(file);

        let mut index = Vec::new();
        let mut consumed: u64 = 0;

        while consumed < index_len {
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf)?;
            let key_len = u32::from_le_bytes(len_buf) as usize;

            let mut key_buf = vec![0u8; key_len];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let mut offset_buf = [0u8; 8];
            reader.read_exact(&mut offset_buf)?;
            let offset = u64::from_le_bytes(offset_buf);

            index.push((key, offset));
            consumed += (4 + key_len + 8) as u64;
        }

        Ok(SSTable {
            index,
            path: path.to_path_buf(),
            index_offset,
        })
    }

    // binary search the index and get the value, one seek one read
    pub fn get(&self, key: &str) -> io::Result<Lookup> {
        let offset = match self.index.binary_search_by(|(k, _)| k.as_str().cmp(key)) {
            Ok(i) => self.index[i].1,
            Err(_) => return Ok(Lookup::NotFound),
        };

        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset + 4 + key.len() as u64))?;

        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf)?;
        let value_len = u32::from_le_bytes(len_buf);

        if value_len == TOMBSTONE {
            return Ok(Lookup::Deleted);
        }

        let mut value_buf = vec![0u8; value_len as usize];
        file.read_exact(&mut value_buf)?;
        let value = String::from_utf8(value_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Lookup::Found(value))
    }

    // used for compaction later
    pub fn iter_entries(&self) -> io::Result<SSTableIter> {
        let file = File::open(&self.path)?;
        Ok(SSTableIter {
            reader: BufReader::new(file),
            remaining: self.index_offset,
        })
    }
}

impl SSTableIter {
    fn read_entry(&mut self) -> io::Result<(String, Option<String>)> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf)?;
        let key_len = u32::from_le_bytes(len_buf) as usize;

        let mut key_buf = vec![0u8; key_len];
        self.reader.read_exact(&mut key_buf)?;
        let key = String::from_utf8(key_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        self.reader.read_exact(&mut len_buf)?;
        let value_len = u32::from_le_bytes(len_buf);

        let value = if value_len == TOMBSTONE {
            None
        } else {
            let mut value_buf = vec![0u8; value_len as usize];
            self.reader.read_exact(&mut value_buf)?;
            Some(
                String::from_utf8(value_buf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            )
        };

        let value_bytes = value.as_ref().map_or(0, |v| v.len());
        self.remaining = self
            .remaining
            .saturating_sub((4 + key_len + 4 + value_bytes) as u64);

        Ok((key, value))
    }
}

impl Iterator for SSTableIter {
    type Item = io::Result<(String, Option<String>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        match self.read_entry() {
            Ok(entry) => Some(Ok(entry)),
            Err(e) => {
                self.remaining = 0;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sstable_test_{name}.sst"))
    }

    fn expect_found(lookup: Lookup) -> String {
        match lookup {
            Lookup::Found(value) => value,
            Lookup::Deleted => panic!("expected Found, got Deleted"),
            Lookup::NotFound => panic!("expected Found, got NotFound"),
        }
    }

    #[test]
    fn test_flush_then_read_back_every_key() {
        let path = temp_path("readback");

        let mut mt = MemTable::new();
        mt.put("a".to_string(), "1".to_string());
        mt.put("b".to_string(), "2".to_string());
        mt.put("c".to_string(), "3".to_string());
        let sst = SSTable::flush_from_memtable(&mt, &path).unwrap();

        assert_eq!(expect_found(sst.get("a").unwrap()), "1");
        assert_eq!(expect_found(sst.get("b").unwrap()), "2");
        assert_eq!(expect_found(sst.get("c").unwrap()), "3");

        std::fs::remove_file(&path).unwrap();
    }

    // the btreemap does the sorting, but the flush has to preserve it or the
    // index stops being binary searchable
    #[test]
    fn test_entries_are_sorted_regardless_of_insert_order() {
        let path = temp_path("sorted");

        let mut mt = MemTable::new();
        for key in ["m", "a", "z", "d", "q"] {
            mt.put(key.to_string(), "v".to_string());
        }
        SSTable::flush_from_memtable(&mt, &path).unwrap();

        let keys: Vec<String> = SSTable::open(&path)
            .unwrap()
            .iter_entries()
            .unwrap()
            .map(|entry| entry.unwrap().0)
            .collect();
        assert_eq!(keys, vec!["a", "d", "m", "q", "z"]);

        std::fs::remove_file(&path).unwrap();
    }

    // Deleted stops the read path, NotFound tells it to keep looking in older
    // files. Collapsing them here resurrects the key.
    #[test]
    fn test_tombstone_reads_back_as_deleted() {
        let path = temp_path("tombstone");

        let mut mt = MemTable::new();
        mt.put("gone".to_string(), "value".to_string());
        mt.delete("gone");
        SSTable::flush_from_memtable(&mt, &path).unwrap();

        let sst = SSTable::open(&path).unwrap();
        assert!(matches!(sst.get("gone").unwrap(), Lookup::Deleted));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_absent_key_reads_back_as_not_found() {
        let path = temp_path("absent");

        let mut mt = MemTable::new();
        mt.put("b".to_string(), "2".to_string());
        mt.put("d".to_string(), "4".to_string());
        let sst = SSTable::flush_from_memtable(&mt, &path).unwrap();

        assert!(matches!(sst.get("a").unwrap(), Lookup::NotFound));
        assert!(matches!(sst.get("c").unwrap(), Lookup::NotFound));
        assert!(matches!(sst.get("z").unwrap(), Lookup::NotFound));

        std::fs::remove_file(&path).unwrap();
    }

    // the footer is the only way back to the index, so an off by one in the
    // offset math shows up here and nowhere else
    #[test]
    fn test_open_round_trips_a_flushed_file() {
        let path = temp_path("roundtrip");

        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "value1".to_string());
        mt.put("key2".to_string(), "value2".to_string());
        mt.delete("key3");
        let written = SSTable::flush_from_memtable(&mt, &path).unwrap();

        let reopened = SSTable::open(&path).unwrap();
        assert_eq!(written.index, reopened.index);
        assert_eq!(written.index_offset, reopened.index_offset);
        assert_eq!(expect_found(reopened.get("key2").unwrap()), "value2");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_iter_entries_stops_before_the_index_block() {
        let path = temp_path("scan");

        let mut mt = MemTable::new();
        mt.put("a".to_string(), "1".to_string());
        mt.put("b".to_string(), "2".to_string());
        mt.delete("c");
        SSTable::flush_from_memtable(&mt, &path).unwrap();

        let entries: Vec<(String, Option<String>)> = SSTable::open(&path)
            .unwrap()
            .iter_entries()
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();

        assert_eq!(
            entries,
            vec![
                ("a".to_string(), Some("1".to_string())),
                ("b".to_string(), Some("2".to_string())),
                ("c".to_string(), None),
            ]
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_empty_memtable_flushes_and_opens() {
        let path = temp_path("empty");

        let mt = MemTable::new();
        SSTable::flush_from_memtable(&mt, &path).unwrap();

        let sst = SSTable::open(&path).unwrap();
        assert!(sst.index.is_empty());
        assert_eq!(sst.iter_entries().unwrap().count(), 0);
        assert!(matches!(sst.get("anything").unwrap(), Lookup::NotFound));

        std::fs::remove_file(&path).unwrap();
    }
}
