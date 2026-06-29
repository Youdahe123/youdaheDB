// Run all tests:        cargo test
// Run one layer:        cargo test wal
// Run future stubs:     cargo test -- --ignored

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
    #[test] #[ignore] fn memtable_put_and_get() { todo!() }
    #[test] #[ignore] fn memtable_returns_none_for_missing_key() { todo!() }
    #[test] #[ignore] fn memtable_delete_marks_tombstone() { todo!() }
    #[test] #[ignore] fn memtable_flushes_when_full() { todo!() }
}

mod sstable_tests {
    #[test] #[ignore] fn sstable_writes_sorted_keys() { todo!() }
    #[test] #[ignore] fn sstable_bloom_filter_rejects_missing_key() { todo!() }
    #[test] #[ignore] fn sstable_binary_search_finds_key() { todo!() }
}

mod lsm_tests {
    #[test] #[ignore] fn lsm_write_then_read() { todo!() }
    #[test] #[ignore] fn lsm_reads_newest_version_of_key() { todo!() }
    #[test] #[ignore] fn lsm_compaction_removes_old_versions() { todo!() }
}

mod raft_tests {
    #[test] #[ignore] fn raft_leader_election() { todo!() }
    #[test] #[ignore] fn raft_log_replication_to_followers() { todo!() }
    #[test] #[ignore] fn raft_leader_failover() { todo!() }
}

mod sharding_tests {
    #[test] #[ignore] fn consistent_hash_routes_to_correct_shard() { todo!() }
    #[test] #[ignore] fn adding_node_remaps_minimal_keys() { todo!() }
}

mod transaction_tests {
    #[test] #[ignore] fn two_phase_commit_succeeds() { todo!() }
    #[test] #[ignore] fn occ_aborts_on_conflict() { todo!() }
}
