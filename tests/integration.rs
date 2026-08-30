// Tests. Only the WAL exists, so only the WAL is tested — each layer gets its
// own module here as it's written. Run `cargo test`, or `cargo test wal`.

mod wal_tests {
    use rustdb::lsm::{Wal, WalRecord, WalOperation};

    #[test]
    fn wal_creates_file() {
        let path = "/tmp/rustdb_test_wal_creates.log";
        assert!(Wal::open(path).is_ok());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn wal_append_and_recover_put() {
        let path = "/tmp/rustdb_test_wal_put.log";
        std::fs::remove_file(path).ok();

        {
            let mut wal = Wal::open(path).unwrap();
            wal.append(&WalRecord {
                key: "name".to_string(),
                value: "youdahe".to_string(),
                operation: WalOperation::Put,
            }).unwrap();
        } // wal dropped here, file handle closed

        let records = Wal::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "name");
        assert_eq!(records[0].value, "youdahe");
        assert!(matches!(records[0].operation, WalOperation::Put));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn wal_append_and_recover_delete() {
        let path = "/tmp/rustdb_test_wal_delete.log";
        std::fs::remove_file(path).ok();

        {
            let mut wal = Wal::open(path).unwrap();
            wal.append(&WalRecord {
                key: "name".to_string(),
                value: "".to_string(),
                operation: WalOperation::Delete,
            }).unwrap();
        }

        let records = Wal::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "name");
        assert!(matches!(records[0].operation, WalOperation::Delete));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn wal_recover_multiple_records() {
        let path = "/tmp/rustdb_test_wal_multi.log";
        std::fs::remove_file(path).ok();

        {
            let mut wal = Wal::open(path).unwrap();
            wal.append(&WalRecord { key: "a".to_string(), value: "1".to_string(), operation: WalOperation::Put }).unwrap();
            wal.append(&WalRecord { key: "b".to_string(), value: "2".to_string(), operation: WalOperation::Put }).unwrap();
            wal.append(&WalRecord { key: "a".to_string(), value: "".to_string(),  operation: WalOperation::Delete }).unwrap();
        }

        let records = Wal::recover(path).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].key, "a");
        assert_eq!(records[1].key, "b");
        assert!(matches!(records[2].operation, WalOperation::Delete));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn wal_empty_file_returns_empty_vec() {
        let path = "/tmp/rustdb_test_wal_empty.log";
        std::fs::remove_file(path).ok();
        Wal::open(path).unwrap();
        let records = Wal::recover(path).unwrap();
        assert_eq!(records.len(), 0);
        std::fs::remove_file(path).ok();
    }
}

mod memtable_tests {
    use rustdb::lsm::{Lookup, MemTable};

    // pulls the string out of a Found, failing loudly on anything else — keeps
    // the assertions below readable
    fn expect_found(lookup: Lookup) -> String {
        match lookup {
            Lookup::Found(value) => value,
            Lookup::Deleted => panic!("expected Found, got Deleted"),
            Lookup::NotFound => panic!("expected Found, got NotFound"),
        }
    }

    #[test]
    fn memtable_put_and_get() {
        let mut table = MemTable::new();
        table.put("name".to_string(), "youdahe".to_string());
        assert_eq!(expect_found(table.get("name")), "youdahe");
    }

    #[test]
    fn memtable_missing_key_is_not_found() {
        let table = MemTable::new();
        assert!(matches!(table.get("nothing"), Lookup::NotFound));
    }

    // overwriting keeps only the newest value — the memtable holds current
    // state, not history (the WAL is what keeps history)
    #[test]
    fn memtable_overwrite_returns_newest_value() {
        let mut table = MemTable::new();
        table.put("name".to_string(), "old".to_string());
        table.put("name".to_string(), "new".to_string());
        assert_eq!(expect_found(table.get("name")), "new");
    }

    // the important one: a delete must leave a tombstone, NOT remove the key.
    // Deleted means "stop searching"; NotFound would let the read fall through
    // to an older sstable and resurrect the value.
    #[test]
    fn memtable_delete_leaves_a_tombstone() {
        let mut table = MemTable::new();
        table.put("name".to_string(), "youdahe".to_string());
        table.delete("name".to_string());
        assert!(matches!(table.get("name"), Lookup::Deleted));
    }

    // deleting a key this memtable never saw still has to record a tombstone —
    // that key may live in an sstable below, and this is the marker that
    // shadows it
    #[test]
    fn memtable_delete_of_unseen_key_still_tombstones() {
        let mut table = MemTable::new();
        table.delete("never-written".to_string());
        assert!(matches!(table.get("never-written"), Lookup::Deleted));
    }

    // a delete is not permanent: writing the key again brings it back to life
    #[test]
    fn memtable_put_after_delete_revives_key() {
        let mut table = MemTable::new();
        table.put("name".to_string(), "youdahe".to_string());
        table.delete("name".to_string());
        table.put("name".to_string(), "back".to_string());
        assert_eq!(expect_found(table.get("name")), "back");
    }

    // keys come out sorted regardless of insert order — this is why the
    // memtable is a BTreeMap, and what makes the sstable flush a straight
    // sequential write with no sort step
    #[test]
    fn memtable_keeps_keys_sorted() {
        let mut table = MemTable::new();
        for key in ["zebra", "apple", "mango"] {
            table.put(key.to_string(), "v".to_string());
        }
        assert_eq!(expect_found(table.get("apple")), "v");
        assert_eq!(expect_found(table.get("zebra")), "v");
    }
}
