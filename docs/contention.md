# Engine contention measurement

Measured September 6, 2026 on an Apple M1 with 8 GiB RAM, macOS 14.6.1,
Homebrew Rust 1.98.0, release profile. These are local development-machine
measurements, with three sequential repetitions of each scenario.

```bash
cargo test --release contention_benchmark -- --ignored --nocapture
```

Defaults: 5,000 operations per scenario, 16,384 keys, 256-byte values, three
repetitions. The fixture contains about 4 MiB of values plus keys and index.
A fresh SSTable copy seeds each scenario, with an empty memtable and WAL.
Fixture construction, copying, database opening, and thread creation are
outside timing. The worker barrier, workload bookkeeping, and scheduling are
included in throughput; individual latency clocks surround get/put only.
Read-result validation happens after that operation's latency is recorded.

The engine scenarios call public Engine methods. Test-only instrumentation
inside their lock helpers records acquisition time in thread-local storage;
normal builds do not contain this instrumentation. Acquisition time includes
the lock call and clock overhead, not just time parked by the OS. The direct
baseline calls LsmTree on one thread with no thread lock, so its reported wait
is zero. Both paths retain the same WAL synchronization and on-disk format.

Operation choices are a fixed permutation with the requested mix per 100
operations; keys follow a fixed stride. Threads partition the operation list,
so order between threads varies. This is a repeatable input set, not a
randomized or statistically representative workload. Read-only and 90% read /
10% overwrite scenarios are measured at 1, 8, and 16 engine threads. The
direct baseline precedes the engine scenarios each time.

## Results

Throughput summarizes the three runs. Latency columns are **medians of the
three per-run p99s**, not a pooled p99. Read and write latency are separate.
The [raw CSV](contention.csv) includes every run's p50, p95, and p99. Its total
throughput column is shared by both operation kinds; it is not per-kind QPS.

| API | Threads | Reads | Ops/sec min / median / max | Read p99 (µs) | Write p99 (µs) | Read / write lock wait p99 (µs) |
|---|---|---|---|---|---|---|
| direct | 1 | 100% | 38198 / 40478 / 40553 | 173.500 | — | 0.000 / — |
| engine | 1 | 100% | 40263 / 40765 / 40806 | 172.750 | — | 0.042 / — |
| engine | 8 | 100% | 66935 / 69239 / 69541 | 933.459 | — | 0.208 / — |
| engine | 16 | 100% | 63728 / 65975 / 70886 | 1525.542 | — | 0.209 / — |
| direct | 1 | 90% | 2389 / 2465 / 2467 | 257.917 | 6183.292 | 0.000 / 0.000 |
| engine | 1 | 90% | 2328 / 2392 / 2396 | 373.667 | 6628.500 | 0.084 / 0.125 |
| engine | 8 | 90% | 2394 / 2465 / 2472 | 20727.791 | 38058.916 | 20468.709 / 32907.917 |
| engine | 16 | 90% | 2468 / 2597 / 2602 | 34745.458 | 61747.917 | 34671.959 / 58021.292 |

## What this supports

Read-only throughput increased from roughly 40,000 operations/sec on one
thread to 66,000–69,000 on 8–16 threads, while individual read tail latency
increased. The one-thread direct and shared read measurements overlap; the
small difference is not evidence that a lock speeds up reads.

Mixed throughput stayed near 2,400–2,600 operations/sec, while median per-run
write p99 reached about 62 ms on 16 threads and write lock-wait p99 reached
about 58 ms. That supports retaining the coarse lock as a correctness baseline
and measuring group commit separately. It does not justify promising higher
write throughput from additional threads or immediately splitting locks.

## Limits

This dataset fits in RAM; OS caching is uncontrolled. Overwrites bring keys
into the memtable, so mixed and read-only workloads use different read paths.
Default timed overwrites do not reach the memtable flush threshold. No scan,
compaction, network, cold-cache, or crash workload is measured. Fixed execution
order, laptop background activity, scheduler decisions, and clock overhead
can affect results. Three repetitions expose variation but do not establish
statistical significance. The direct baseline is not a competing database,
and there is no Mutex baseline to claim RwLock is universally superior.

Increase YOUDAHEDB_BENCH_KEYS, YOUDAHEDB_BENCH_VALUE_BYTES,
YOUDAHEDB_BENCH_OPS, and YOUDAHEDB_BENCH_REPEATS for other workloads. The
harness rejects zero or malformed settings. This scoped contention test does
not replace issue #14's broader storage benchmark and amplification metrics.
