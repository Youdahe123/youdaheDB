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
