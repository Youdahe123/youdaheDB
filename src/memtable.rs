use std::collections::BTreeMap;

// in memory sorted key value store
pub struct MemTable {
    entries: BTreeMap<String, Option<String>>,
    size_bytes: usize,
}

impl MemTable {
    pub fn new() -> Self {
        MemTable {
            entries: BTreeMap::new(),
            size_bytes: 0,
        }
    }

    pub fn put(&mut self, key: String, value: String) {
        self.size_bytes += key.len() + value.len();
        self.entries.insert(key, Some(value));
    }

    // store tombstone instead of removing
    pub fn delete(&mut self, key: &str) {
        self.size_bytes += key.len();
        self.entries.insert(key.to_string(), None);
    }

    pub fn get(&self, key: &str) -> Option<&Option<String>> {
        self.entries.get(key)
    }

    pub fn size(&self) -> usize {
        self.size_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Option<String>)> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.size_bytes = 0;
    }
}
