// Storage engine — the bottom of the database, the only layer that touches
// disk. Everything above (server, sharding, raft) ends up calling into here.
//
// Implemented: the WAL. Every write is appended and fsynced here BEFORE it
// touches memory, so a crash can only lose writes that were never acked.
// Replaying it on startup rebuilds what memory held at crash time.
// Record layout: [ op (1) ][ key_len (4) ][ key ][ val_len (4) ][ value ]
//
// Still to write: MemTable (sorted in-memory buffer), SSTable (immutable
// sorted file), BloomFilter (skip a file without reading it), LsmTree (ties
// them together + compaction).

use std::fs::OpenOptions;
use std::io::Write;
use std::io::Read;
use std::io::BufReader;

// the two operations a WAL record can represent
pub enum WalOperation {
    Put,
    Delete,
}

// one entry in the WAL — captures a single database operation completely
pub struct WalRecord {
    pub key: String,
    pub value: String,
    pub operation: WalOperation, // removed operation layer so tests could pass
    
}

// holds an open handle to the WAL file on disk
pub struct Wal {
    file: std::fs::File,
}

impl Wal {

    // opens the WAL file at the given path, creating it if it doesn't exist
    // returns a Wal wrapping the open file handle, or an io error
    pub fn open(path: &str) -> Result<Wal, std::io::Error> {
        let file = OpenOptions::new()
            .read(true)    // needed for recover
            .append(true)  // all writes go to the end, never overwrite
            .create(true)  // create the file if it doesn't exist yet
            .open(path)?;  // ? returns the error immediately if open fails

        Ok(Wal { file })
    }

    // serializes one WalRecord to disk in this format:
    // [ op (1 byte) ][ key_len (4 bytes) ][ key bytes ][ value_len (4 bytes) ][ value bytes ]
    pub fn append(&mut self, record: &WalRecord) -> Result<(), std::io::Error> {
        // convert the operation enum to a single byte: Put=1, Delete=2
        let op_byte = match record.operation {
            WalOperation::Put => 1u8,
            WalOperation::Delete => 2u8,
        };

        self.file.write_all(&[op_byte])?;

        // write key length as exactly 4 bytes (little-endian) so recover knows how many bytes to read
        self.file.write_all(&(record.key.len() as u32).to_le_bytes())?;
        // write the raw key bytes
        self.file.write_all(record.key.as_bytes())?;

        // same pattern for the value
        self.file.write_all(&(record.value.len() as u32).to_le_bytes())?;
        self.file.write_all(record.value.as_bytes())?;

        // force the OS to flush its buffer to physical disk — without this, durability isn't guaranteed
        self.file.sync_all()?;
        Ok(())
    }

    // reads the WAL file from beginning to end and reconstructs all WalRecords
    // used on startup to rebuild the MemTable after a crash
    pub fn recover(path: &str) -> Result<Vec<WalRecord>, std::io::Error> {
        let file = std::fs::File::open(path)?;
        // BufReader batches disk reads for efficiency instead of one syscall per byte
        let mut reader = BufReader::new(file);

        let mut records: Vec<WalRecord> = Vec::new();

        loop {
            // try to read the op byte — if we hit end of file, we're done
            let mut op_buf = [0u8; 1];
            if reader.read_exact(&mut op_buf).is_err() {
                break;
            }

            // convert the byte back to a WalOperation — reverse of what append wrote
            let operation = match op_buf[0] {
                1 => WalOperation::Put,
                2 => WalOperation::Delete,
                _ => break, // unexpected byte, stop reading
            };

            // read 4 bytes for key length, convert back to usize for use as a buffer size
            let mut key_len_buf = [0u8; 4];
            reader.read_exact(&mut key_len_buf)?;
            let key_len = u32::from_le_bytes(key_len_buf) as usize;

            // read exactly key_len bytes and convert back to a String
            let mut key_buf = vec![0u8; key_len];
            reader.read_exact(&mut key_buf)?;
            let key = String::from_utf8(key_buf).unwrap();

            // same pattern for the value
            let mut value_len_buf = [0u8; 4];
            reader.read_exact(&mut value_len_buf)?;
            let value_len = u32::from_le_bytes(value_len_buf) as usize;

            let mut value_buf = vec![0u8; value_len];
            reader.read_exact(&mut value_buf)?;
            let value = String::from_utf8(value_buf).unwrap();

            // push the fully reconstructed record into the list
            records.push(WalRecord { key, value, operation });
        }

        Ok(records)
    }

    // safe only once the memtable's contents are durable in an sstable —
    // truncating before the flush lands turns a crash into data loss
    pub fn truncate(&mut self) -> Result<(), std::io::Error> {
        self.file.set_len(0)?;
        self.file.sync_all()
    }
}
