// TODO: drop this allow once the stubs at the bottom of the file are implemented
#![allow(unused_variables, dead_code)]

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
    pub operation: WalOperation,
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
}

// ─────────────────────────────────────────────────────────────────────────────
// Below: plumbing is implemented, todo!() marks a DESIGN DECISION that's yours.
// Each todo!() says which decision it's waiting on.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use std::io::{BufWriter, Seek, SeekFrom};

// deterministic hash — DefaultHasher (unlike HashMap's RandomState) produces the
// same output across process restarts, which is required for both the bloom
// filter on disk and shard routing to stay stable
pub fn stable_hash(key: &str, seed: u64) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish()
}

// result of looking a key up in ONE layer (memtable or a single sstable)
// the distinction matters: Deleted means "stop searching older layers",
// NotFound means "keep going, an older layer might have it"
pub enum Lookup {
    Found(String),
    Deleted,
    NotFound,
}

// shared on-disk entry encoding, used by both SSTables and (in the same shape)
// the WAL: [ op (1) ][ key_len (4) ][ key ][ val_len (4) ][ val ]
// op 1 = live value, op 2 = tombstone
pub fn write_entry<W: Write>(w: &mut W, key: &str, value: &Option<String>) -> Result<(), std::io::Error> {
    let (op, val) = match value {
        Some(v) => (1u8, v.as_str()),
        None => (2u8, ""),
    };
    w.write_all(&[op])?;
    w.write_all(&(key.len() as u32).to_le_bytes())?;
    w.write_all(key.as_bytes())?;
    w.write_all(&(val.len() as u32).to_le_bytes())?;
    w.write_all(val.as_bytes())?;
    Ok(())
}

// reads one entry back. Ok(None) means a clean end of file.
pub fn read_entry<R: Read>(r: &mut R) -> Result<Option<(String, Option<String>)>, std::io::Error> {
    let mut op_buf = [0u8; 1];
    if r.read_exact(&mut op_buf).is_err() {
        return Ok(None);
    }

    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let mut key_buf = vec![0u8; u32::from_le_bytes(len_buf) as usize];
    r.read_exact(&mut key_buf)?;
    let key = String::from_utf8_lossy(&key_buf).into_owned();

    r.read_exact(&mut len_buf)?;
    let mut val_buf = vec![0u8; u32::from_le_bytes(len_buf) as usize];
    r.read_exact(&mut val_buf)?;
    let value = String::from_utf8_lossy(&val_buf).into_owned();

    match op_buf[0] {
        1 => Ok(Some((key, Some(value)))),
        _ => Ok(Some((key, None))),
    }
}

// ─── MemTable ────────────────────────────────────────────────────────────────
// in-memory write buffer. BTreeMap (not HashMap) because flushing to an SSTable
// requires the keys to come out already sorted.
// value is Option<String>: Some(v) = live value, None = tombstone (deleted)
pub struct MemTable {
    map: BTreeMap<String, Option<String>>,
    size_bytes: usize,
    max_size_bytes: usize,
}

impl MemTable {
    pub fn new(max_size_bytes: usize) -> MemTable {
        MemTable { map: BTreeMap::new(), size_bytes: 0, max_size_bytes }
    }

    pub fn put(&mut self, key: String, value: String) {
        self.insert(key, Some(value));
    }

    // a tombstone still occupies a slot, so it can shadow older values
    // living in the sstables below
    pub fn delete(&mut self, key: String) {
        self.insert(key, None);
    }

    // overwriting a key replaces its bytes rather than adding to them, so the
    // old entry's size comes back off the running total
    fn insert(&mut self, key: String, value: Option<String>) {
        let added = Self::entry_size(&key, &value);
        let removed = self.map.get(&key).map_or(0, |old| Self::entry_size(&key, old));
        self.map.insert(key, value);
        self.size_bytes = self.size_bytes + added - removed;
    }

    fn entry_size(key: &str, value: &Option<String>) -> usize {
        key.len() + value.as_ref().map_or(0, |v| v.len())
    }

    pub fn get(&self, key: &str) -> Lookup {
        match self.map.get(key) {
            Some(Some(value)) => Lookup::Found(value.clone()),
            Some(None) => Lookup::Deleted,
            None => Lookup::NotFound,
        }
    }

    // the flush trigger
    pub fn is_full(&self) -> bool {
        self.size_bytes >= self.max_size_bytes
    }

    // entries, tombstones included
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn size_bytes(&self) -> usize {
        self.size_bytes
    }

    // sorted iteration — what SSTable::flush_from_memtable consumes
    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, String, Option<String>> {
        self.map.iter()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.size_bytes = 0;
    }
}

// ─── BloomFilter ─────────────────────────────────────────────────────────────
// probabilistic set: "definitely not here" or "maybe here", never a false
// negative. Lets a read skip an sstable entirely without touching disk.
pub struct BloomFilter {
    bits: Vec<u64>,
    num_bits: usize,
    num_hashes: u32,
}

impl BloomFilter {
    // ~10 bits/item gives roughly a 1% false-positive rate.
    // optimal hash count k = (m/n) * ln2, which is bits_per_item * ln2
    pub fn new(expected_items: usize, bits_per_item: usize) -> BloomFilter {
        let requested = (expected_items * bits_per_item).max(64);
        let num_words = requested.div_ceil(64);
        let num_hashes = ((bits_per_item as f64) * std::f64::consts::LN_2).round().max(1.0) as u32;
        BloomFilter { bits: vec![0u64; num_words], num_bits: num_words * 64, num_hashes }
    }

    pub fn insert(&mut self, key: &str) {
        for pos in self.bit_positions(key) {
            self.bits[pos / 64] |= 1u64 << (pos % 64);
        }
    }

    // false = definitely absent; true = MIGHT be present
    pub fn may_contain(&self, key: &str) -> bool {
        self.bit_positions(key)
            .into_iter()
            .all(|pos| self.bits[pos / 64] & (1u64 << (pos % 64)) != 0)
    }

    // double hashing: derives k positions from two base hashes instead of
    // running k independent hash functions. |1 keeps the step odd so it can
    // reach every slot rather than cycling through a subset.
    fn bit_positions(&self, key: &str) -> Vec<usize> {
        let h1 = stable_hash(key, 0);
        let h2 = stable_hash(key, 0x9E37_79B9_7F4A_7C15) | 1;
        (0..self.num_hashes)
            .map(|i| (h1.wrapping_add((i as u64).wrapping_mul(h2)) % self.num_bits as u64) as usize)
            .collect()
    }

    // [ num_hashes (4) ][ num_words (4) ][ words... ] — stored in the sstable footer
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.bits.len() * 8);
        out.extend_from_slice(&self.num_hashes.to_le_bytes());
        out.extend_from_slice(&(self.bits.len() as u32).to_le_bytes());
        for word in &self.bits {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> BloomFilter {
        let num_hashes = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let num_words = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let bits = (0..num_words)
            .map(|i| {
                let start = 8 + i * 8;
                u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap())
            })
            .collect();
        BloomFilter { bits, num_bits: num_words * 64, num_hashes }
    }
}

// ─── SSTable ─────────────────────────────────────────────────────────────────
// an immutable, sorted-on-disk run of key/value entries.
// layout: [ entries... ][ index block ][ bloom block ][ footer: index_off, bloom_off ]
pub struct SSTable {
    pub path: String,
    // (key, byte offset of that key's entry) — kept in memory
    index: Vec<(String, u64)>,
    bloom: BloomFilter,
}

impl SSTable {
    // DESIGN: how dense is the index? One entry per key is simplest and is what
    // seek_offset() below assumes. A *sparse* index (every Nth key) uses far less
    // memory but then a lookup has to scan forward from the preceding key.
    // Pick one, then write the entries, index, bloom, and footer in that order.
    pub fn flush_from_memtable(memtable: &MemTable, path: &str) -> Result<SSTable, std::io::Error> {
        let file = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(file);
        let mut index: Vec<(String, u64)> = Vec::new();
        let mut bloom = BloomFilter::new(memtable.len().max(1), 10);
        let mut offset: u64 = 0;

        for (key, value) in memtable.iter() {
            let bytes = Self::encode_entry(key, value);
            index.push((key.clone(), offset));
            bloom.insert(key);
            writer.write_all(&bytes)?;
            offset += bytes.len() as u64;
        }

        todo!("write the index block, the bloom block, and the footer, then \
               fsync and return the SSTable with its path, index and bloom. \
               Footer layout is your call — it just has to let open() find both blocks.")
    }

    // loads only the index + bloom into memory; entries stay on disk
    pub fn open(path: &str) -> Result<SSTable, std::io::Error> {
        todo!("read the footer to locate the index and bloom blocks, then \
               rebuild them — the mirror image of flush_from_memtable")
    }

    // DESIGN: the read path is bloom check -> binary search the index -> ONE
    // seek + read. Every step exists to avoid disk work; skipping the bloom
    // still gives correct answers, just slower.
    pub fn get(&self, key: &str) -> Result<Lookup, std::io::Error> {
        if !self.bloom.may_contain(key) {
            return Ok(Lookup::NotFound);
        }

        let offset = match self.seek_offset(key) {
            Some(offset) => offset,
            None => return Ok(Lookup::NotFound),
        };

        let mut file = std::fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;

        match read_entry(&mut file)? {
            Some((found, Some(value))) if found == key => Ok(Lookup::Found(value)),
            Some((found, None)) if found == key => Ok(Lookup::Deleted),
            _ => Ok(Lookup::NotFound),
        }
    }

    // full sorted scan — what compaction merges over
    pub fn iter_entries(&self) -> Result<Vec<(String, Option<String>)>, std::io::Error> {
        let file = std::fs::File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        // stops at the index block rather than the end of file, since the index
        // and bloom follow the entries in the same file
        let entry_bytes_end = self.index.last().map_or(0, |(_, off)| *off);
        let mut consumed = 0u64;

        while consumed <= entry_bytes_end {
            match read_entry(&mut reader)? {
                Some((key, value)) => {
                    consumed += Self::encode_entry(&key, &value).len() as u64;
                    entries.push((key, value));
                }
                None => break,
            }
        }

        Ok(entries)
    }

    fn encode_entry(key: &str, value: &Option<String>) -> Vec<u8> {
        let mut buf = Vec::new();
        write_entry(&mut buf, key, value).expect("writing to a Vec cannot fail");
        buf
    }

    // exact hit, or the slot just before the target; None if the target sorts
    // before every key in this run
    fn seek_offset(&self, key: &str) -> Option<u64> {
        match self.index.binary_search_by(|(k, _)| k.as_str().cmp(key)) {
            Ok(i) => Some(self.index[i].1),
            Err(0) => None,
            Err(i) => Some(self.index[i - 1].1),
        }
    }
}

// ─── LsmTree ─────────────────────────────────────────────────────────────────
// ties it together: writes go WAL -> memtable, reads go memtable -> newest
// sstable -> oldest
pub struct LsmTree {
    dir: String,
    wal: Wal,
    memtable: MemTable,
    // newest first, so a linear scan naturally hits the freshest version first
    sstables: Vec<SSTable>,
    next_sstable_id: u64,
}

// memtable size at which a flush is triggered
pub const DEFAULT_MEMTABLE_BYTES: usize = 4 * 1024 * 1024;

impl LsmTree {
    pub fn open(dir: &str) -> Result<LsmTree, std::io::Error> {
        std::fs::create_dir_all(dir)?;
        let wal = Wal::open(&format!("{dir}/wal.log"))?;
        let sstables = Self::load_sstables(dir)?;
        let next_sstable_id = sstables.len() as u64;

        let mut tree = LsmTree {
            dir: dir.to_string(),
            wal,
            memtable: MemTable::new(DEFAULT_MEMTABLE_BYTES),
            sstables,
            next_sstable_id,
        };

        tree.replay_wal()?;
        Ok(tree)
    }

    // DESIGN: the ordering here IS the durability guarantee. WAL append (with
    // its fsync) must complete BEFORE the memtable mutation — a crash between
    // the two loses a write you already acknowledged. Then decide whether a
    // full memtable flushes synchronously here or gets handed to a background
    // thread (synchronous is simpler and fine for this project).
    pub fn put(&mut self, key: &str, value: &str) -> Result<(), std::io::Error> {
        todo!("append to self.wal, then self.memtable.put, then flush if is_full")
    }

    pub fn delete(&mut self, key: &str) -> Result<(), std::io::Error> {
        todo!("same order as put(), but writes a tombstone")
    }

    // DESIGN: read path order is what makes the LSM correct. Newest layer wins,
    // and a Deleted result must STOP the search — falling through to an older
    // sstable would resurrect a deleted key.
    pub fn get(&self, key: &str) -> Result<Option<String>, std::io::Error> {
        todo!("check self.memtable, then self.sstables in newest-first order; \
               Found -> Some(v), Deleted -> None, NotFound -> keep looking")
    }

    // DESIGN: the WAL can only be truncated once the sstable is durably on
    // disk — truncating first turns a crash into data loss.
    pub fn flush(&mut self) -> Result<(), std::io::Error> {
        todo!("SSTable::flush_from_memtable into dir/sstable-<next_id>.sst, push \
               it onto the FRONT of self.sstables, bump next_sstable_id, clear \
               the memtable, then truncate the WAL")
    }

    // DESIGN: this is where write amplification vs. read amplification gets
    // traded. Merging everything into one run (what the signature assumes) makes
    // reads fastest and rewrites the most data; leveled or tiered compaction
    // rewrites less but leaves more runs to search.
    // Also: a tombstone can only be dropped once no older run still holds the
    // key — dropping it early resurrects the value.
    pub fn compact(&mut self) -> Result<(), std::io::Error> {
        todo!("k-way merge self.sstables newest-first, keeping the first version \
               seen of each key, then replace self.sstables with the merged run")
    }

    // rebuilds whatever the memtable held at crash time
    fn replay_wal(&mut self) -> Result<(), std::io::Error> {
        let records = Wal::recover(&format!("{}/wal.log", self.dir))?;
        for record in records {
            match record.operation {
                WalOperation::Put => self.memtable.put(record.key, record.value),
                WalOperation::Delete => self.memtable.delete(record.key),
            }
        }
        Ok(())
    }

    // newest id first, matching the order self.sstables is searched in
    fn load_sstables(dir: &str) -> Result<Vec<SSTable>, std::io::Error> {
        let mut paths: Vec<String> = std::fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path().to_string_lossy().into_owned())
            .filter(|path| path.ends_with(".sst"))
            .collect();

        // filenames are sstable-<id>.sst, so a reverse lexical sort is only
        // correct while ids are zero-padded — sort by parsed id instead
        paths.sort_by_key(|path| std::cmp::Reverse(Self::sstable_id(path)));
        paths.iter().map(|path| SSTable::open(path)).collect()
    }

    fn sstable_id(path: &str) -> u64 {
        path.rsplit("sstable-")
            .next()
            .and_then(|tail| tail.strip_suffix(".sst"))
            .and_then(|id| id.parse().ok())
            .unwrap_or(0)
    }
}

impl Wal {
    // safe only once the memtable's contents are durable in an sstable
    pub fn truncate(&mut self) -> Result<(), std::io::Error> {
        self.file.set_len(0)?;
        self.file.sync_all()
    }
}
