//! Opt-in measurement of the public API. Instrumentation exists only in test builds.
use super::{tests::TestDir, Engine};
use crate::{lsm::LsmTree, memtable::MemTable, sstable::SSTable};
use std::cell::Cell;
use std::io;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

thread_local! { static WAIT: Cell<Duration> = const { Cell::new(Duration::ZERO) }; }
pub(super) fn record_wait(wait: Duration) {
    WAIT.with(|slot| slot.set(wait));
}

#[derive(Default)]
struct Samples {
    reads: Vec<(u128, u128)>,
    writes: Vec<(u128, u128)>,
}

fn setting(name: &str, default: usize) -> usize {
    let value = std::env::var(name).map_or(default, |s| {
        s.parse().expect("benchmark setting must be an integer")
    });
    assert!(value > 0, "{name} must be positive");
    value
}

fn percentile(samples: &mut [u128], p: usize) -> f64 {
    samples.sort_unstable();
    samples[(samples.len() - 1) * p / 100] as f64 / 1000.0
}

fn report(
    mode: &str,
    repeat: usize,
    threads: usize,
    read_pct: usize,
    elapsed: f64,
    samples: Samples,
) {
    let ops = samples.reads.len() + samples.writes.len();
    for (kind, group) in [("read", samples.reads), ("write", samples.writes)] {
        if group.is_empty() {
            continue;
        }
        let count = group.len();
        let (mut latency, mut wait): (Vec<_>, Vec<_>) = group.into_iter().unzip();
        println!("{mode},{repeat},{threads},{read_pct},{kind},{count},{:.0},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            ops as f64 / elapsed,
            percentile(&mut latency, 50), percentile(&mut latency, 95), percentile(&mut latency, 99),
            percentile(&mut wait, 50), percentile(&mut wait, 95), percentile(&mut wait, 99));
    }
}

fn workload(
    ops: usize,
    id: usize,
    threads: usize,
    read_pct: usize,
    keys: &[String],
    mut execute: impl FnMut(bool, &str) -> io::Result<Option<String>>,
) -> Samples {
    let mut samples = Samples {
        reads: Vec::with_capacity(ops / threads + 1),
        writes: Vec::with_capacity(ops / threads + 1),
    };
    for i in (id..ops).step_by(threads) {
        // Fixed permutation of operation choices avoids a burst of 90 reads
        // followed by 10 writes. Every full 100 operations has the requested mix.
        let read = i.wrapping_mul(37) % 100 < read_pct;
        let key = &keys[i.wrapping_mul(7919) % keys.len()];
        WAIT.with(|slot| slot.set(Duration::ZERO));
        let start = Instant::now();
        let result = execute(read, key);
        let latency = start.elapsed().as_nanos();
        let wait = WAIT.with(|slot| slot.get().as_nanos());
        let value = result.unwrap();
        if read {
            assert!(value.is_some());
        }
        let group = if read {
            &mut samples.reads
        } else {
            &mut samples.writes
        };
        group.push((latency, wait));
    }
    samples
}

#[test]
#[ignore = "run in release mode with --ignored --nocapture"]
fn contention_benchmark() {
    let ops = setting("YOUDAHEDB_BENCH_OPS", 5000);
    let key_count = setting("YOUDAHEDB_BENCH_KEYS", 16384);
    let value_bytes = setting("YOUDAHEDB_BENCH_VALUE_BYTES", 256);
    let repeats = setting("YOUDAHEDB_BENCH_REPEATS", 3);
    let keys: Arc<Vec<String>> = Arc::new((0..key_count).map(|k| format!("k{k:08}")).collect());
    let value = "v".repeat(value_bytes);
    // Build one immutable fixture outside timing, rather than fsync every seed
    // write. Each scenario receives a fresh copy and an empty memtable/WAL.
    let fixture = TestDir::new();
    let fixture_path = fixture.0.join("sst-000000.sst");
    let mut memtable = MemTable::new();
    for key in keys.iter() {
        memtable.put(key.clone(), value.clone());
    }
    SSTable::flush_from_memtable(&memtable, &fixture_path).unwrap();
    drop(memtable);
    println!("config: ops={ops}, keys={key_count}, value_bytes={value_bytes}, repeats={repeats}");
    println!("mode,repeat,threads,read_pct,kind,count,total_ops_per_sec,p50_us,p95_us,p99_us,wait_p50_us,wait_p95_us,wait_p99_us");
    for repeat in 1..=repeats {
        for read_pct in [100, 90] {
            let baseline = TestDir::new();
            std::fs::copy(&fixture_path, baseline.0.join("sst-000000.sst")).unwrap();
            let mut tree = LsmTree::open(baseline.0.to_str().unwrap()).unwrap();
            let start = Instant::now();
            let samples = workload(ops, 0, 1, read_pct, &keys, |read, key| {
                if read {
                    tree.get(key)
                } else {
                    tree.put(key, &value).map(|_| None)
                }
            });
            report(
                "direct",
                repeat,
                1,
                read_pct,
                start.elapsed().as_secs_f64(),
                samples,
            );
            for threads in [1, 8, 16] {
                let dir = TestDir::new();
                std::fs::copy(&fixture_path, dir.0.join("sst-000000.sst")).unwrap();
                let db = Engine::open(dir.0.to_str().unwrap()).unwrap();
                let start_barrier = Arc::new(Barrier::new(threads + 1));
                let handles: Vec<_> = (0..threads)
                    .map(|id| {
                        let db = db.clone();
                        let keys = keys.clone();
                        let value = value.clone();
                        let barrier = start_barrier.clone();
                        std::thread::spawn(move || {
                            barrier.wait();
                            let samples =
                                workload(ops, id, threads, read_pct, &keys, |read, key| {
                                    if read {
                                        db.get(key)
                                    } else {
                                        db.put(key, &value).map(|_| None)
                                    }
                                });
                            (samples, Instant::now())
                        })
                    })
                    .collect();
                let start = Instant::now();
                start_barrier.wait();
                let mut end = start;
                let mut samples = Samples::default();
                for handle in handles {
                    let (mut worker, finished) = handle.join().unwrap();
                    end = end.max(finished);
                    samples.reads.append(&mut worker.reads);
                    samples.writes.append(&mut worker.writes);
                }
                report(
                    "engine",
                    repeat,
                    threads,
                    read_pct,
                    (end - start).as_secs_f64(),
                    samples,
                );
            }
        }
    }
}
