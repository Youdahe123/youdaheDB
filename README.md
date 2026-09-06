<div align="center">

# youdaheDB

### A distributed database engine built from scratch in Rust

**Multi-Region Consensus · LSM-Tree Storage · ACID Transactions**

[![Write-up](https://img.shields.io/badge/Read-Algorythm_Article-FF6719?style=flat-square)](https://news.algorythm.org/p/building-a-distributed-database-from)
[![Report](https://img.shields.io/badge/Read-Engineering_Report-1f6feb?style=flat-square)](https://youdahe123.github.io/pdf/project1_distributed_database.pdf)
[![Rust](https://img.shields.io/badge/Rust-std_only-000000?style=flat-square&logo=rust)](https://www.rust-lang.org/)

</div>

---

A horizontally scalable database engine inspired by CockroachDB and Google Spanner: a custom Log-Structured Merge Tree storage engine, Raft-based consensus for fault-tolerant replication, a SQL-compatible query layer with latency-aware multi-region routing, ACID transactions via two-phase commit and optimistic concurrency control, and automatic resharding using consistent hashing — with configurable strong, eventual, and causal consistency.

Built layer by layer, in the open, with zero external crates.

> *Many engineers use databases every day. Few ever look inside one.*

## Media

| | |
|---|---|
| 📰 **[Building a Distributed Database From Scratch](https://news.algorythm.org/p/building-a-distributed-database-from)** | Part I of a series in *Algorythm*, a community of 20k+ Black software engineers |
| 📄 **[Distributed Database Engine — Engineering Report](https://youdahe123.github.io/pdf/project1_distributed_database.pdf)** | The full 7-page system design: architecture, consensus, transactions, sharding, testing |
| 🔗 **[More projects →](https://youdahe123.github.io/index.html)** | Other infrastructure work — from-scratch replicas of Docker, Kubernetes, and more |

---

## System architecture

Five layers, each owning one concern in the data lifecycle.

```
┌──────────────────────────────────────────────────────────────────────┐
│  CLIENT LAYER                                                        │
│  SQL-compatible wire protocol  ·  key-value API                      │
└───────────────────────────────┬──────────────────────────────────────┘
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  QUERY ROUTING LAYER                                                  │
│  Parser & planner  ·  latency-aware router  ·  txn coordinator (2PC)  │
└───────────────────────────────┬──────────────────────────────────────┘
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  CONSENSUS LAYER — one Raft group per shard                          │
│  Leader election  ·  log replication  ·  quorum commit  ·  snapshots  │
└───────────────────────────────┬──────────────────────────────────────┘
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  STORAGE ENGINE — LSM-tree                                     ◄ HERE │
│  Write-ahead log  ·  MemTable  ·  SSTable levels  ·  compaction       │
└───────────────────────────────┬──────────────────────────────────────┘
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│  SHARDING & MULTI-REGION REPLICATION                                 │
│  Consistent hashing (256 vnodes)  ·  auto-rebalancing  ·  CRDTs       │
└──────────────────────────────────────────────────────────────────────┘
   Consistency: Strong | Eventual | Causal
```

**Where the code is today:** the storage engine, from the bottom up. The write-ahead log and MemTable are built and tested; SSTables, compaction and everything above them are designed in the [engineering report](https://youdahe123.github.io/pdf/project1_distributed_database.pdf) and tracked as [open issues](https://github.com/Youdahe123/youdaheDB/issues).

---

## Try it

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
```

Now kill the process mid-session and start it again:

```
$ cargo run

recovered 2 entries from WAL
db> get user:1
youdahe
```

Nothing was flushed to a data file. Nothing was gracefully shut down. The data came back because every write reached disk *before* it was ever acknowledged.

That single guarantee is what the rest of the engine is built around.

---

## Sharing the engine between threads

The Rust executable and library use `Engine`, a clonable handle sharing one
`Arc<RwLock<LsmTree>>`. Open a data directory once, then clone the handle:

```rust
use youdaheDB::Engine;

fn main() -> std::io::Result<()> {
    let db = Engine::open("data")?;
    let writer = db.clone();
    std::thread::spawn(move || writer.put("user:1", "alice"))
        .join().expect("writer panicked")?;
    assert_eq!(db.get("user:1")?, Some("alice".to_string()));
    Ok(())
}
```

`get`, `scan`, and `sstable_count` share a read lock. `put`, `delete`, and
`flush` take the exclusive write lock, including WAL synchronization and any
automatic flush. Multiple readers can run together; a writer excludes all
other operations. Scheduling fairness is determined by the platform's
`RwLock`; no bounded writer-wait guarantee is made.

`scan()` returns `io::Result<Vec<(String, String)>>`: it collects a sorted,
consistent view under the read lock and releases the lock before returning.
Writers wait during collection, and result memory grows with the live dataset.
Processing the returned rows holds no lock. If a writer panics while holding
the lock, subsequent operations return an `io::Error` identifying the poisoned
lock. Individual operations are synchronized; a sequence of calls is not a
transaction.

This API coordinates threads sharing one handle. It does not lock a directory
against independent `Engine::open` calls or other processes. It does not add
background flushes, group commit, or stronger crash guarantees to the existing
storage engine.

### Contention measurement

Run the opt-in benchmark (excluded from normal tests):

```bash
cargo test --release contention_benchmark -- --ignored --nocapture
# Optional: change the total operations per scenario (default 10000)
YOUDAHEDB_BENCH_OPS=50000 cargo test --release contention_benchmark -- --ignored --nocapture
```

Each scenario starts with 256 keys, 128-byte values, and one flushed SSTable
in a temporary directory. It executes a deterministic sequence of reads or
90% reads / 10% overwrites across 1, 8, and 16 threads. Setup is excluded from
timing. The test measures the same lock helpers and tree operations as
`Engine::get`/`put`, separating lock acquisition from total operation latency
without adding production instrumentation. It reports p50/p95/p99 for both.
Thread scheduling and measured times are not deterministic.

Example local run, September 6, 2026: Apple M1, 8 GiB RAM, macOS 14.6.1,
Homebrew Rust 1.98.0, release build, 10,000 operations per scenario:

| Threads | Reads | Ops/sec | Operation p50 / p95 / p99 (µs) | Lock wait p50 / p95 / p99 (µs) |
|---|---|---|---|---|
| 1 | 100% | 63,605 | 14.959 / 15.792 / 23.417 | 0.042 / 0.042 / 0.042 |
| 8 | 100% | 115,986 | 59.083 / 129.875 / 207.833 | 0.042 / 0.125 / 0.250 |
| 16 | 100% | 121,405 | 54.416 / 590.292 / 1,152.791 | 0.042 / 0.166 / 0.250 |
| 1 | 90% | 2,634 | 0.291 / 3,965.875 / 4,078.708 | 0.042 / 0.125 / 0.250 |
| 8 | 90% | 2,672 | 1.041 / 14,948.375 / 39,900.792 | 0.125 / 12,147.792 / 38,910.625 |
| 16 | 90% | 2,688 | 1.208 / 30,907.000 / 76,078.959 | 0.166 / 29,960.750 / 75,788.583 |

In this run, concurrent reads increased throughput, while mixed throughput
stayed roughly flat and tail lock waits grew. Every overwrite synchronizes
the WAL under the write lock. These are illustrative single-run measurements
on a development laptop, not a comparison with another database or a speedup
claim over the old implementation. The small read dataset benefits from OS
caching; overwrites move keys into the memtable, so the two workloads have
different read paths. There is no network, compaction, or timed flush workload.
Repeat on your hardware before choosing finer locks or group commit.

## Storage engine

### The write path

```
   put("user:1", "youdahe")
             │
             ▼
   ┌───────────────────┐
   │  Write-Ahead Log  │   append + fsync — durable before anything else
   └─────────┬─────────┘
             │
             ▼
   ┌───────────────────┐
   │     MemTable      │   sorted, in RAM — instant
   └─────────┬─────────┘
             │
             ▼
        ack the client
```

Every write is serialized and appended to the WAL, a sequential append-only file. Only once that write is confirmed durable is it applied to the in-memory MemTable. When the MemTable crosses its size threshold it is frozen, a fresh one takes over incoming writes, and the frozen one is flushed to disk as an immutable SSTable.

### The read path

```
   get("user:1")
             │
             ▼
       MemTable ──── Found ────► return value
             │
        NotFound          Deleted ──► return "not found", STOP
             │
             ▼
    SSTables, newest → oldest
        └─ bloom filter first: "definitely not here" skips the file with zero I/O
```

Reads traverse the hierarchy newest to oldest and stop at the first answer. At each SSTable a Bloom filter is consulted before any disk I/O — at roughly a 1% false-positive rate, that eliminates the large majority of unnecessary reads. Range scans use a merge iterator over a priority queue across all levels, yielding keys in sorted order.

### Write-Ahead Log

An append-only file where every operation is recorded before it is applied anywhere else. Think of a receipt printer: the kitchen gets the ticket before the order is prepared. If the process dies, you replay the tickets and reconstruct exactly where you left off.

Appending is `O(1)` and needs no seek, which is why a WAL is fast despite touching disk on every write — sequential disk writes are cheap, random ones are not.

**On-disk format** — length-prefixed, little-endian, no framing beyond the lengths:

```
[ key_len: u32 ][ key bytes ][ val_len: u32 ][ value bytes ]
```

A delete writes `u32::MAX` in the value-length slot as a tombstone sentinel, so replay can tell a deletion from a zero-length string.

**Why a log can represent a map.** It doesn't, directly — it represents *history*. Replay it front to back and later records overwrite earlier ones; the final state is the map. That is also why replay is idempotent: re-running the same history twice lands on the same result, which is what makes crash recovery safe to retry.

### MemTable

The engine's live in-memory view of the newest data. Backed by a `BTreeMap`, not a `HashMap`, and the choice is deliberate: a hash map is marginally faster for the point lookups a memtable mostly serves, but it destroys key order. Sorted order buys two things — range scans by key prefix, and a flush that is a straight sequential write with no sort step.

The MemTable tracks its own size so a flush fires before RAM is exhausted. Overwriting a key *replaces* its bytes rather than adding to them, so the counter reflects current contents rather than everything ever written — otherwise a workload hammering a few hot keys would flush far too early.

*The engineering report specifies a concurrent skip list for lock-free reads under concurrency; the current single-threaded implementation uses a `BTreeMap`, which has the same ordering guarantees.*

### Tombstones

The piece that makes deletes correct, and the one that is easiest to get wrong.

Deleting a key cannot mean removing it from the MemTable. Older copies may already sit in files on disk. Remove the entry and a read falls straight through to those files, finds the old value, and returns it — **the deleted key comes back**.

So a delete is a *write*. It stores a `None` at that key: a marker that occupies a real slot and shadows everything beneath it. Which means a lookup in any single layer has three outcomes, not two:

| Result | Meaning | What the read path does |
|---|---|---|
| `Found(v)` | live value in this layer | return it |
| `Deleted` | tombstone in this layer | return "not found" and **stop searching** |
| `NotFound` | this layer has never seen the key | keep searching older layers |

Collapse the last two into a single `Option::None` — the obvious API — and the engine loses the ability to distinguish *"this key is deleted"* from *"I don't know about this key"*. One means stop; the other means keep going. That distinction is the whole reason deletes work.

In an LSM-tree, deleting data always makes the database bigger. It only gets smaller during compaction.

### Compaction

As SSTables accumulate, every read miss searches more files and superseded values are never reclaimed. Leveled compaction merges SSTables from Level N into the overlapping tables at Level N+1: duplicate keys resolve to their newest version, tombstones are garbage collected, and the output is written with fresh Bloom filters and indexes. Compaction is rate-limited so background merging cannot starve foreground I/O.

A tombstone may only be dropped once **no older run can still hold that key**. Drop it early and the delete is undone.

---

## The invariants

Ordering rules the engine is built on. Each is one line of code, and each one, reversed, produces a bug that stays invisible until the worst possible moment.

**1. WAL append and fsync completes before the MemTable is touched.**
Reverse it and a crash in the gap loses a write that was already acknowledged. The client was told "OK" for data that no longer exists.

**2. A flushed SSTable is durable before the WAL is cleared.**
Reverse it and a crash in that window destroys data that exists in neither place — not in the log, not yet in the file.

**3. A tombstone halts the read path.**
Fall through one into an older layer and deleted keys resurrect.

**4. The hash must be stable across restarts.**
Rust's default `HashMap` hasher is seeded per process. A Bloom filter written today would reject its own keys tomorrow — a silent read failure no single-process test can catch. The same rule governs shard routing: an unstable hash relocates every key on restart.

---

## Beyond the storage engine

Designed in the [engineering report](https://youdahe123.github.io/pdf/project1_distributed_database.pdf), not yet implemented.

### Consensus — Raft, one group per shard

Per-shard isolation means consensus overhead scales with the number of *active* shards rather than total cluster size.

Followers hold a randomized election timeout (150–300ms). Miss a heartbeat and a follower becomes a candidate, increments the term, votes for itself, and requests votes from its peers; a majority wins. Randomization keeps split votes rare and short-lived.

The leader takes all writes, assigns each a monotonically increasing log index, and replicates via AppendEntries. An entry commits once a majority has durably stored it. **Because commit quorums and election quorums always overlap, any newly elected leader is guaranteed to have seen every previously committed entry** — that overlap is the safety proof, not a rule to memorize.

Periodic snapshots cap unbounded log growth; joint consensus makes membership changes safe against split-brain.

### Transactions — 2PC, OCC, HLC

**Two-phase commit** coordinates transactions spanning multiple Raft groups. In the prepare phase each shard takes locks, validates constraints, and writes a prepare record to its WAL. Unanimous yes and the coordinator writes a commit decision, then broadcasts. The decision record is what makes crash recovery possible — on restart, in-doubt transactions resolve by reading the coordinator's log.

**Optimistic concurrency control** suits read-heavy, low-contention workloads: transactions read freely while recording the versions they touched, and validation at commit time checks whether anything changed underneath them. Pass and the writes apply atomically; fail and the transaction retries with fresh reads.

**Hybrid Logical Clocks** give causally consistent timestamps without GPS or atomic clocks. A physical component tracks wall time with bounded drift; a logical counter breaks ties when physical stamps collide, yielding a total order that respects causality.

### Query layer & routing

SQL over a PostgreSQL-compatible wire protocol. An LALR parser builds an AST; a cost-based planner uses data distribution statistics, index availability and estimated cardinalities to choose between scan strategies, join algorithms and execution order.

The router keeps a live map of shard locations and measured round-trip times:

| Consistency | Served from | Trade |
|---|---|---|
| **Strong** | the Raft leader for that shard | always current, serialized through one node |
| **Eventual** | any replica, nearest preferred | fast and parallel, may lag |
| **Causal** | any follower caught up to the required HLC timestamp | reads reflect all causally preceding writes |

### Sharding & multi-region replication

Consistent hashing with 256 virtual nodes per physical node, so a node joining or leaving remaps only a proportional fraction of keys rather than reshuffling the entire keyspace. Each shard replicates across a configurable number of regions (default 3) under placement constraints — for example, at least one replica each in US-East, US-West and EU-West.

Rebalancing triggers when shard sizes diverge past a threshold: oversized shards split at their midpoint key, undersized neighbours merge. Reads continue from existing replicas throughout. Eventually consistent replicas resolve conflicts with CRDTs for commutative operations and vector clocks for detecting genuinely conflicting writes.

### Observability & testing

Prometheus metrics at every layer — compaction rates and amplification factors, Raft election frequency and replication lag, query latency percentiles, transaction commit/abort ratios — with OpenTelemetry trace context propagated end to end.

Correctness is verified by Jepsen-style chaos testing: network partitions, clock skew, process crashes and disk failures injected while concurrent clients run transactions, followed by linearizability analysis of the committed history. No committed data lost, no stale reads under strong consistency.

---

## Status

| Layer | State | Tests |
|---|---|---|
| Write-Ahead Log — append, replay, clear | ✅ built | 6 |
| MemTable — sorted store, tombstones, size tracking | ✅ built | 7 |
| REPL — put / get / delete / scan, recovery on start | ✅ built | — |
| SSTable — flush to immutable sorted files | 🔜 [#2](https://github.com/Youdahe123/youdaheDB/issues/2) | |
| Storage engine — coordinate WAL + MemTable + SSTables | 🔜 [#3](https://github.com/Youdahe123/youdaheDB/issues/3) | |
| Compaction — merge runs, reclaim tombstones | 🔜 [#4](https://github.com/Youdahe123/youdaheDB/issues/4) | |
| Bloom filters — skip files without reading them | 🔜 [#5](https://github.com/Youdahe123/youdaheDB/issues/5) | |
| Raft consensus, transactions, sharding, query layer | 📐 designed | |

```bash
cargo test     # full suite
./test.sh      # layer-by-layer status
```

Every WAL write is `fsync`'d before it is acknowledged — `flush()` alone only reaches the OS page cache, which survives a process crash but not power loss. That `sync_all()` is also the single slowest line in the engine; batching it across concurrent writers (group commit) is the standard way to buy it back.

---

## Why Rust

The borrow checker eliminates whole classes of systems bugs — use-after-free, data races — at compile time rather than in production.

It does not solve coordination for you. When one thread serves reads while another flushes the MemTable to disk, Rust will not decide whether that handoff wants a `Mutex`, an `RwLock` or a channel; you still have to reason about the access pattern. What it does is make it far harder to turn a wrong answer into undefined behaviour. In Java or Python you find out in production. In Rust, many of those mistakes simply don't compile.

And no garbage collector means predictable latency. Halfway through flushing a MemTable or replaying a log is not where you want a GC pause.

---

## What building this teaches

Things no amount of `SELECT * FROM` will:

- **Why WALs exist** — sequential appends are fast, random writes are not
- **Why tombstones exist** — you cannot simply erase something when older copies live on disk
- **Why LSM-trees flush and compact** — memory is fast and finite, disk is slow and abundant, and reads degrade without maintenance
- **Why consensus is hard** — getting three machines to agree on anything, across crashes and partitions, is a genuinely difficult problem

Zero external dependencies. A `Cargo.toml`, the standard library, and a lot of binary serialization.

youdaheDB is not production-ready, and is not trying to be.

---

## About

**Youdahe Asfaw** — Computer Science at Gustavus Adolphus College. Distributed systems, infrastructure, reliability engineering. Founder of Docere; ML researcher; builds from-scratch replicas of tools like Docker and Kubernetes.

**[youdahe123.github.io](https://youdahe123.github.io/index.html)** · [LinkedIn](https://linkedin.com/in/youdaheasfaw)
