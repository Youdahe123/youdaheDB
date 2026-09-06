use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "youdahedb-engine-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn open(&self) -> Engine {
        Engine::open(self.0.to_str().unwrap()).unwrap()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn engine_is_send_and_sync() {
    fn assert_traits<T: Send + Sync>() {}
    assert_traits::<Engine>();
}

#[test]
fn readers_overlap_and_exclude_a_writer() {
    let dir = TestDir::new();
    let db = dir.open();
    db.put("key", "value").unwrap();
    let held = db.read().unwrap();
    let other = db.clone();
    let (send, recv) = mpsc::channel();
    let worker = thread::spawn(move || {
        send.send(other.get("key")).unwrap();
    });
    // The first read guard is still alive when the second reader completes.
    let result = recv.recv_timeout(Duration::from_secs(5));
    assert!(matches!(
        db.inner.try_write(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    drop(held);
    worker.join().unwrap();
    assert_eq!(result.unwrap().unwrap(), Some("value".into()));
    assert!(db.inner.try_write().is_ok());
}

#[test]
fn scan_results_are_owned_and_release_the_lock() {
    let dir = TestDir::new();
    let db = dir.open();
    db.put("a", "old").unwrap();
    db.flush().unwrap();
    db.put("b", "deleted").unwrap();
    db.delete("b").unwrap();
    let snapshot = db.scan().unwrap();
    assert!(db.inner.try_write().is_ok());
    db.put("a", "new").unwrap();
    assert_eq!(snapshot, vec![("a".into(), "old".into())]);
    assert_eq!(db.scan().unwrap(), vec![("a".into(), "new".into())]);
}

#[test]
fn poisoned_writer_returns_errors_from_every_operation() {
    let dir = TestDir::new();
    let db = dir.open();
    let other = db.clone();
    assert!(thread::spawn(move || {
        let _guard = other.write().unwrap();
        panic!("simulate a panic during mutation");
    })
    .join()
    .is_err());
    let errors = [
        db.get("k").unwrap_err(),
        db.put("k", "v").unwrap_err(),
        db.delete("k").unwrap_err(),
        db.scan().unwrap_err(),
        db.flush().unwrap_err(),
        db.sstable_count().unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("poisoned"));
    }
}

#[test]
fn concurrent_reads_writes_deletes_and_flushes_survive_reopen() {
    const WRITERS: usize = 4;
    const READERS: usize = 4;
    const KEYS: usize = 32;
    let dir = TestDir::new();
    let db = dir.open();
    let start = Arc::new(Barrier::new(WRITERS + READERS));
    thread::scope(|scope| {
        for writer in 0..WRITERS {
            let db = db.clone();
            let start = start.clone();
            scope.spawn(move || {
                start.wait();
                for n in 0..KEYS {
                    let key = format!("{writer}:{n:03}");
                    db.put(&key, "old").unwrap();
                    db.put(&key, &key).unwrap();
                    if n % 2 == 0 {
                        db.delete(&key).unwrap();
                    }
                    if n % 8 == 0 {
                        db.flush().unwrap();
                    }
                }
            });
        }
        for _ in 0..READERS {
            let db = db.clone();
            let start = start.clone();
            scope.spawn(move || {
                start.wait();
                for _ in 0..KEYS {
                    if let Some(value) = db.get("0:001").unwrap() {
                        assert!(value == "old" || value == "0:001");
                    }
                    let rows = db.scan().unwrap();
                    assert!(rows.windows(2).all(|w| w[0].0 < w[1].0));
                    assert!(rows.iter().all(|(k, v)| v == "old" || k == v));
                }
            });
        }
    });
    let expected: Vec<_> = (0..WRITERS)
        .flat_map(|w| {
            (0..KEYS).filter(|n| n % 2 == 1).map(move |n| {
                let key = format!("{w}:{n:03}");
                (key.clone(), key)
            })
        })
        .collect();
    assert_eq!(db.scan().unwrap(), expected);
    assert!(db.sstable_count().unwrap() > 0);
    drop(db);
    let reopened = dir.open();
    assert_eq!(reopened.scan().unwrap(), expected);
    for w in 0..WRITERS {
        for n in (0..KEYS).step_by(2) {
            assert_eq!(reopened.get(&format!("{w}:{n:03}")).unwrap(), None);
        }
    }
}

fn percentile(samples: &mut [u128], percentile: usize) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() - 1) * percentile / 100]
}

/// Explicit benchmark, excluded from CI's normal correctness suite.
/// Times the same lock helpers and tree operations used by Engine get/put,
/// allowing lock acquisition to be measured without production instrumentation.
#[test]
#[ignore = "run in release mode with --ignored --nocapture"]
fn contention_benchmark() {
    let ops: usize = std::env::var("YOUDAHEDB_BENCH_OPS")
        .unwrap_or_else(|_| "10000".into())
        .parse()
        .expect("positive operation count");
    assert!(ops > 0);
    println!("threads,read_pct,ops,ops_per_sec,latency_p50_us,latency_p95_us,latency_p99_us,wait_p50_us,wait_p95_us,wait_p99_us");
    for read_pct in [100, 90] {
        for threads in [1, 8, 16] {
            let dir = TestDir::new();
            let db = dir.open();
            let value = "v".repeat(128);
            for key in 0..256 {
                db.put(&format!("k{key:03}"), &value).unwrap();
            }
            db.flush().unwrap();
            let barrier = Arc::new(Barrier::new(threads + 1));
            let handles: Vec<_> = (0..threads)
                .map(|id| {
                    let db = db.clone();
                    let barrier = barrier.clone();
                    let value = value.clone();
                    thread::spawn(move || {
                        let mut samples = Vec::new();
                        barrier.wait();
                        for i in (id..ops).step_by(threads) {
                            let key = format!("k{:03}", i.wrapping_mul(73) % 256);
                            let start = Instant::now();
                            let wait;
                            if i % 100 < read_pct {
                                let guard = db.read().unwrap();
                                wait = start.elapsed().as_nanos();
                                assert_eq!(
                                    guard.get(&key).unwrap().as_deref(),
                                    Some(value.as_str())
                                );
                            } else {
                                let mut guard = db.write().unwrap();
                                wait = start.elapsed().as_nanos();
                                guard.put(&key, &value).unwrap();
                            }
                            samples.push((start.elapsed().as_nanos(), wait));
                        }
                        samples
                    })
                })
                .collect();
            let start = Instant::now();
            barrier.wait();
            let samples: Vec<_> = handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect();
            let elapsed = start.elapsed().as_secs_f64();
            let (mut latency, mut wait): (Vec<_>, Vec<_>) = samples.into_iter().unzip();
            println!(
                "{threads},{read_pct},{ops},{:.0},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
                ops as f64 / elapsed,
                percentile(&mut latency, 50) as f64 / 1000.0,
                percentile(&mut latency, 95) as f64 / 1000.0,
                percentile(&mut latency, 99) as f64 / 1000.0,
                percentile(&mut wait, 50) as f64 / 1000.0,
                percentile(&mut wait, 95) as f64 / 1000.0,
                percentile(&mut wait, 99) as f64 / 1000.0
            );
        }
    }
}
