use std::collections::BTreeMap;

// sorted in memory key value store, sits on top of the wal
// keys are always sorted because of btreemap which makes flushing to disk efficient
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "value1".to_string());

        match mt.get("key1") {
            Some(Some(v)) => assert_eq!(v, "value1"),
            _ => panic!("expected value1"),
        }
        assert!(mt.get("key2").is_none());
    }

    #[test]
    fn test_delete_creates_tombstone() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "value1".to_string());
        mt.delete("key1");

        match mt.get("key1") {
            Some(None) => {} // tombstone
            _ => panic!("expected tombstone"),
        }
    }

    #[test]
    fn test_overwrite() {
        let mut mt = MemTable::new();
        mt.put("key1".to_string(), "v1".to_string());
        mt.put("key1".to_string(), "v2".to_string());

        match mt.get("key1") {
            Some(Some(v)) => assert_eq!(v, "v2"),
            _ => panic!("expected v2"),
        }
    }

    #[test]
    fn test_sorted_iteration() {
        let mut mt = MemTable::new();
        mt.put("c".to_string(), "3".to_string());
        mt.put("a".to_string(), "1".to_string());
        mt.put("b".to_string(), "2".to_string());

        let keys: Vec<&String> = mt.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }
}
