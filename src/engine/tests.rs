use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Barrier};
use std::thread;
use std::time::Duration;

pub(super) struct TestDir(pub(super) PathBuf);

impl TestDir {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "youdahedb-engine-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create test directory: {e}"),
            }
        }
    }
    fn open(&self) -> Engine {
        Engine::open(self.0.to_str().unwrap()).unwrap()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "could not clean test directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn rows(db: &Engine) -> Vec<(String, String)> {
    db.scan()
        .unwrap()
        .iter()
        .unwrap()
        .collect::<io::Result<_>>()
        .unwrap()
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
fn scan_blocks_writer_and_early_drop_allows_progress() {
    let dir = TestDir::new();
    let db = dir.open();
    db.put("a", "old").unwrap();
    db.put("b", "old").unwrap();
    db.flush().unwrap();
    let scan = db.scan().unwrap();
    let mut entries = scan.iter().unwrap();
    assert_eq!(entries.next().unwrap().unwrap(), ("a".into(), "old".into()));
    let other = db.clone();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let blocked = matches!(
            other.inner.try_write(),
            Err(std::sync::TryLockError::WouldBlock)
        );
        attempt_tx.send(blocked).unwrap();
        done_tx.send(other.put("b", "new")).unwrap();
    });
    assert!(attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    // Re-iterating this guard acquires no recursive lock. The queued writer
    // cannot change the view between its first and last row.
    assert_eq!(
        scan.iter()
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap(),
        vec![("a".into(), "old".into()), ("b".into(), "old".into())]
    );
    drop(entries); // abandon the scan before its last row
    assert!(matches!(
        db.inner.try_write(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    drop(scan);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(db.get("b").unwrap(), Some("new".into()));
}

#[test]
fn scan_streams_rows_before_a_later_disk_error_and_releases_on_drop() {
    let dir = TestDir::new();
    let db = dir.open();
    db.put("a", "first").unwrap();
    db.put("b", "corrupt-me").unwrap();
    db.flush().unwrap();
    let path = dir.0.join("sst-000000.sst");
    let mut bytes = fs::read(&path).unwrap();
    let offset = bytes
        .windows(b"corrupt-me".len())
        .position(|b| b == b"corrupt-me")
        .unwrap();
    bytes[offset] = 0xff; // invalid UTF-8 in the second value, not its index
    fs::write(path, bytes).unwrap();
    {
        let scan = db.scan().unwrap();
        let mut entries = scan.iter().unwrap();
        assert_eq!(
            entries.next().unwrap().unwrap(),
            ("a".into(), "first".into())
        );
        assert_eq!(
            entries.next().unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    assert!(db.inner.try_write().is_ok());
    db.put("new", "value").unwrap();
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
        db.scan().err().unwrap(),
        db.flush().unwrap_err(),
        db.sstable_count().unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("poisoned"));
    }
}

#[test]
fn competing_writes_deletes_scans_and_flushes_survive_reopen() {
    const WRITERS: usize = 4;
    const READERS: usize = 4;
    const ROUNDS: usize = 24;
    let dir = TestDir::new();
    let db = dir.open();
    let phase = Arc::new(Barrier::new(WRITERS + READERS));
    thread::scope(|scope| {
        for writer in 0..WRITERS {
            let db = db.clone();
            let phase = phase.clone();
            scope.spawn(move || {
                for round in 0..ROUNDS {
                    phase.wait();
                    if round % 2 == 0 {
                        db.put("shared", &format!("{round}:{writer}")).unwrap();
                    } else {
                        db.delete("shared").unwrap();
                    }
                    db.put(&format!("writer:{writer}"), &round.to_string())
                        .unwrap();
                    if writer == 0 {
                        db.flush().unwrap();
                    }
                    phase.wait(); // all writes in this round have completed
                    phase.wait(); // every reader verified this round
                }
            });
        }
        for _ in 0..READERS {
            let db = db.clone();
            let phase = phase.clone();
            scope.spawn(move || {
                for round in 0..ROUNDS {
                    phase.wait();
                    for _ in 0..4 {
                        if let Some(value) = db.get("shared").unwrap() {
                            let visible_round: usize =
                                value.split(':').next().unwrap().parse().unwrap();
                            assert_eq!(
                                visible_round,
                                if round % 2 == 0 { round } else { round - 1 }
                            );
                        }
                        let view = rows(&db);
                        assert!(view.windows(2).all(|w| w[0].0 < w[1].0));
                    }
                    phase.wait();
                    let value = db.get("shared").unwrap();
                    if round % 2 == 0 {
                        assert!(value.unwrap().starts_with(&format!("{round}:")));
                    } else {
                        assert_eq!(value, None);
                        assert!(!rows(&db).iter().any(|(k, _)| k == "shared"));
                    }
                    phase.wait();
                }
            });
        }
    });
    db.put("unflushed", "survives").unwrap();
    let expected = rows(&db);
    for writer in 0..WRITERS {
        assert_eq!(
            db.get(&format!("writer:{writer}")).unwrap(),
            Some((ROUNDS - 1).to_string())
        );
    }
    drop(db);
    let reopened = dir.open();
    assert_eq!(rows(&reopened), expected);
    assert_eq!(reopened.get("shared").unwrap(), None);
    assert_eq!(reopened.get("unflushed").unwrap(), Some("survives".into()));
}

#[test]
fn directory_ownership_lasts_until_the_last_clone_is_dropped() {
    let dir = TestDir::new();
    let db = dir.open();
    let clone = db.clone();
    drop(db);
    let error = Engine::open(dir.0.join(".").to_str().unwrap())
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    clone.put("k", "v").unwrap();
    drop(clone);
    assert!(dir.0.join("LOCK").exists());
    assert_eq!(dir.open().get("k").unwrap(), Some("v".into()));
}

#[test]
fn failed_open_releases_directory_ownership() {
    let dir = TestDir::new();
    let corrupt_table = dir.0.join("sst-000000.sst");
    fs::write(&corrupt_table, b"bad").unwrap();
    assert_eq!(
        Engine::open(dir.0.to_str().unwrap()).err().unwrap().kind(),
        io::ErrorKind::InvalidData
    );
    fs::remove_file(corrupt_table).unwrap();
    let db = dir.open();
    db.put("k", "v").unwrap();
}

#[cfg(unix)]
#[test]
fn a_symlink_does_not_bypass_directory_ownership() {
    let dir = TestDir::new();
    let aliases = TestDir::new();
    let db = dir.open();
    let alias = aliases.0.join("alias");
    std::os::unix::fs::symlink(&dir.0, &alias).unwrap();
    let error = Engine::open(alias.to_str().unwrap()).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    db.put("k", "v").unwrap();
}
