# rustdb

A small, deliberately simplified prototype of a distributed KV store in
Rust — built to demonstrate (not to productionize) the core ideas behind
systems like CockroachDB/TiKV:

- **LSM-tree storage** (`src/lsm.rs`) — memtable + WAL + sorted segments + compaction
- **Raft-style replication** (`src/raft.rs`) — fixed leader, majority-quorum commit, no election
- **Configurable consistency** (`src/consistency.rs`) — `STRONG` (leader) vs `EVENTUAL` (follower, can lag)
- **Automatic sharding** (`src/sharding.rs`) — hash-routed keys across N independent shard clusters
- **CRDTs** (`src/crdt.rs`) — G-Counter / PN-Counter with conflict-free merge

See [REPORT.md](REPORT.md) for what's real vs. simplified, and a benchmark
against Redis with honest caveats. See [LINKEDIN_POST.md](LINKEDIN_POST.md)
for a ready-to-use post draft.

## Run the demo (best for a quick screenshot)

```bash
cargo run --release -- demo
```

## Run the server

```bash
cargo run --release -- server --port 7878 --shards 4 --consistency strong
```

## Benchmark against Redis

```bash
redis-server --save "" --appendonly no &
cargo run --release -- server --port 7878 &
cargo run --release -- bench --ops 20000 --value-size 100
```
