# youdaheDB

A key value database engine built from scratch in Rust using only the standard library. Based on the LSM tree architecture that powers databases like RocksDB and LevelDB.

## What it does

Write key value pairs, read them back, delete them, and scan everything in sorted order. Kill the process and restart it and your data is still there thanks to the write ahead log.

```
$ cargo run
youdaheDB v0.1
commands: put <key> <value> | get <key> | delete <key> | scan | quit

db> put user:1 youdahe
OK
db> put user:2 alice
OK
db> get user:1
youdahe
db> scan
  user:1 = youdahe
  user:2 = alice
db> quit
bye
```

Restart and everything is recovered from the WAL:

```
$ cargo run
recovered 2 entries from WAL
youdaheDB v0.1
db> get user:1
youdahe
```

## Architecture

**Write Ahead Log (WAL)** Every write hits disk before it touches memory. On crash, replay the log to rebuild state. Uses a binary format with length prefixed keys and values. Deletes are stored as tombstones using u32::MAX as a sentinel value.

**MemTable** Sorted in memory key value store backed by a BTreeMap. Writes go here after the WAL. Reads check here first since it has the newest data. Deletes insert a tombstone (None) instead of removing the key so they can shadow older values on disk.

## Built with

Rust standard library only. No external crates.

## Next steps

SSTable (sorted string table) for flushing the memtable to disk, storage engine to coordinate everything, and eventually Raft consensus for replication across nodes.
