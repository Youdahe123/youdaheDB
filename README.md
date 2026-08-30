# youdaheDB

A key-value storage engine written from scratch in Rust — no external crates, no frameworks. Just raw bytes, files, and the LSM-tree architecture that powers RocksDB, LevelDB and Cassandra.

> Many engineers use databases every day. Few ever look inside one.

📖 **[Building a Distributed Database From Scratch](https://news.algorythm.org/p/building-a-distributed-database-from)** — the write-up, in *Algorythm*.

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

Nothing was flushed to a data file. Nothing was gracefully shut down. The data came back because every write was on disk *before* it was ever acknowledged.

That single guarantee is what the rest of this engine is built around.

---

## How a write moves through the system

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
   │     MemTable      │   sorted BTreeMap in RAM — instant
   └─────────┬─────────┘
             │
             ▼
          ack client
```

And a read walks the layers newest to oldest, stopping at the first answer:

```
   get("user:1")
             │
             ▼
       MemTable ──── Found ────► return value
             │
        NotFound          Deleted ──► return "not found", STOP
             │
             ▼
    SSTables, newest → oldest        (roadmap)
```

The ordering in both diagrams is not stylistic. Each one encodes a correctness rule, and reversing either produces a specific, silent bug — described below.

---

## The components

### Write-Ahead Log

An append-only file where every operation is recorded before it is applied anywhere else. Think of a receipt printer: the kitchen gets the ticket before the order is prepared. If the process dies, you replay the tickets and reconstruct exactly where you left off.

Appending is `O(1)` and requires no seek, which is why a WAL is fast despite touching disk on every write — sequential disk writes are cheap, random ones are not.

**On-disk format** — length-prefixed, little-endian, no framing beyond the lengths:

```
[ key_len: u32 ][ key bytes ][ val_len: u32 ][ value bytes ]
```

A delete writes `u32::MAX` in the value-length slot as a tombstone sentinel, so replay can tell a deletion from a zero-length string.

**Why a log can represent a map.** It doesn't, directly — it represents *history*. Replay it front to back and later records overwrite earlier ones; the final state is the map. That's also why replay is idempotent: re-running the same history twice lands on the same result, which is what makes crash recovery safe to retry.

### MemTable

The engine's live in-memory view of the newest data. Backed by a `BTreeMap`, not a `HashMap`, and the choice is deliberate: a hash map would be marginally faster for the point lookups the memtable mostly serves, but it destroys key order. Sorted order buys two things — range scans by key prefix, and a flush to disk that is a straight sequential write with no sort step.

The memtable tracks its own size so a flush can be triggered before it exhausts RAM. Overwriting a key *replaces* its bytes rather than adding to them, so the counter reflects current contents rather than everything ever written — otherwise a workload that hammers a few hot keys would trigger flushes far too early.

### Tombstones

The piece that makes deletes correct, and the one that is easy to get wrong.

Deleting a key cannot mean removing it from the memtable. Older copies of that key may already be sitting in files on disk. Remove the entry and a read falls straight through to those files, finds the old value, and returns it — **the deleted key comes back**.

So a delete is a *write*. It stores a `None` at that key: a marker that occupies a real slot and shadows everything beneath it. Which means a lookup in any single layer has three outcomes, not two:

| Result | Meaning | What the read path does |
|---|---|---|
| `Found(v)` | live value in this layer | return it |
| `Deleted` | tombstone in this layer | return "not found" and **stop searching** |
| `NotFound` | this layer has never seen the key | keep searching older layers |

Collapse the last two into a single `Option::None` — the obvious API — and the engine loses the ability to distinguish *"this key is deleted"* from *"I don't know about this key"*. One means stop; the other means keep going. That distinction is the whole reason deletes work.

In an LSM-tree, deleting data always makes the database bigger. It only gets smaller during compaction.

---

## The invariants

Three ordering rules the engine is built on. Each is one line of code, and each one, reversed, produces a bug that is invisible until the worst possible moment.

**1. WAL append and fsync completes before the memtable is touched.**
Reverse it and a crash in the gap loses a write that was already acknowledged. The client was told "OK" for data that no longer exists.

**2. A flushed SSTable is durable before the WAL is cleared.**
Reverse it and a crash in that window destroys data that exists in neither place — not in the log, not yet in the file.

**3. A tombstone halts the read path.**
Fall through one into an older layer and deleted keys resurrect.

There is also a fourth, waiting for bloom filters: **the hash must be stable across restarts.** Rust's default `HashMap` hasher is seeded per process, so a filter written today would reject its own keys tomorrow — a silent read failure that no single-process test can catch.

---

## Status

| Layer | State | Tests |
|---|---|---|
| Write-Ahead Log — append, replay, clear | ✅ built | 6 |
| MemTable — sorted store, tombstones, size tracking | ✅ built | 7 |
| REPL — put / get / delete / scan, recovery on start | ✅ built | — |
| SSTable — flush to immutable sorted files | 🔜 [#2](https://github.com/Youdahe123/youdaheDB/issues/2) | |
| Storage engine — coordinate WAL + memtable + SSTables | 🔜 [#3](https://github.com/Youdahe123/youdaheDB/issues/3) | |
| Compaction — merge runs, reclaim tombstones | 🔜 [#4](https://github.com/Youdahe123/youdaheDB/issues/4) | |
| Bloom filters — skip files without reading them | 🔜 [#5](https://github.com/Youdahe123/youdaheDB/issues/5) | |
| Raft consensus — replication across nodes | 🔜 planned | |

```bash
cargo test     # full suite
./test.sh      # layer-by-layer status
```

**Known gap:** the WAL currently calls `flush()`, which hands bytes to the OS page cache — that survives a process crash but not power loss. `sync_all()` is what makes it genuinely durable. Fix in flight on `feat/port-testing-stage`.

---

## Roadmap

**Compaction** is the big one. As SSTables accumulate, every read miss has to search more files, and superseded values and tombstones are never reclaimed. Compaction merges runs, keeps the newest version of each key, and drops tombstones — but only once no older run can still hold that key. Drop one early and the delete is undone.

That merge strategy is the central trade in LSM design: merging everything into one run makes reads fastest and rewrites the most data; leveled and tiered compaction rewrite far less but leave more files to search. Write amplification against read amplification.

Beyond it: **checksums** to detect on-disk corruption, a **manifest** so the engine doesn't rediscover its SSTables by scanning a directory at startup, and **WAL rotation** so the log is replaced rather than truncated in place.

Then **Raft**. A single-node database is useful until the node dies. One elected leader takes all writes; an entry commits only once a majority acknowledges it. Because commit quorums and election quorums always overlap, any newly elected leader is guaranteed to have seen every previously committed entry — that overlap *is* the safety proof. Each node keeps its own local log, similar in purpose to the single-node WAL; Raft is the protocol that keeps those independent logs in agreement.

---

## Why Rust

The borrow checker eliminates whole classes of systems bugs — use-after-free, data races — at compile time rather than in production.

It does not, however, solve coordination for you. When one thread serves reads while another flushes the memtable to disk, Rust will not decide whether that handoff wants a `Mutex`, an `RwLock`, or a channel. You still have to reason about the access pattern. What it does is make it much harder to turn a wrong answer into undefined behaviour: in Java or Python you find out in production, in Rust many of those mistakes simply don't compile.

And no garbage collector means predictable latency. Halfway through flushing a memtable or replaying a log is not where you want a GC pause.

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

Built by **Youdahe Asfaw** — CS at Gustavus Adolphus College, founder of [Docere](https://github.com/Youdahe123), ML researcher. Specializes in infrastructure, building from-scratch replicas of tools like Docker and Kubernetes.

Written up in [Algorythm](https://news.algorythm.org/p/building-a-distributed-database-from), a community of 20k+ Black software engineers. Part I of a series.
