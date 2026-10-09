//! Kills the writer mid-write and inspects what survived.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use youdaheDB::Engine;

const VALUE_BYTES: usize = 16384;

// Must match crash_writer's value_for, so a wrong value is caught, not just a missing one.
fn value_for(i: usize, size: usize) -> String {
    let seed = format!("{i}:");
    seed.repeat(size / seed.len() + 1)[..size].to_string()
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("youdahedb-crash-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Kills land in a different place each run, which is the point.
fn jitter(lo_ms: u64, hi_ms: u64) -> Duration {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos() as u64;
    Duration::from_millis(lo_ms + nanos % (hi_ms - lo_ms))
}

fn sstable_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "sst"))
        .count()
}

// Runs the writer for `run_for`, SIGKILLs it, returns every key it acked.
fn kill_mid_write(dir: &Path, run_for: Duration) -> Vec<String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_crash_writer"))
        .arg(dir)
        .arg(VALUE_BYTES.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Drained on its own thread so a full pipe never stalls the writer.
    let acked = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take().unwrap();
    let collector = {
        let acked = Arc::clone(&acked);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                acked.lock().unwrap().push(line.unwrap());
            }
        })
    };

    std::thread::sleep(run_for);

    // Exiting on its own is a real bug, not the crash we staged.
    if let Some(status) = child.try_wait().unwrap() {
        let mut stderr = String::new();
        child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        panic!("writer exited by itself ({status}): {stderr}");
    }

    child.kill().unwrap(); // SIGKILL: no unwinding, no flush, no Drop
    let status = child.wait().unwrap();
    assert!(status.code().is_none(), "expected death by signal, got {status}");

    // Bytes already in the pipe survive the kill, so drain before reading acks.
    collector.join().unwrap();
    Arc::try_unwrap(acked).unwrap().into_inner().unwrap()
}

// The invariant is one-way: every acked write must survive the crash.
// Unacked writes that made it to disk anyway are fine, so they aren't checked.
fn verify_acked(dir: &Path, acked: &[String]) {
    let db = Engine::open(dir.to_str().unwrap())
        .unwrap_or_else(|e| panic!("reopen after kill failed: {e}"));
    for (i, key) in acked.iter().enumerate() {
        match db.get(key).unwrap() {
            Some(v) if v == value_for(i, VALUE_BYTES) => {}
            Some(_) => panic!("{key} survived with the wrong value"),
            None => panic!("{key} was acked but lost ({} acked total)", acked.len()),
        }
    }
}

#[test]
fn acked_writes_survive_a_kill() {
    let dir = TestDir::new("a4");
    let acked = kill_mid_write(&dir.0, jitter(1500, 3500));

    assert!(!acked.is_empty(), "writer acked nothing before the kill");
    for (i, key) in acked.iter().enumerate() {
        assert_eq!(key, &format!("key:{i}"), "acks must arrive in order, no gaps");
    }

    verify_acked(&dir.0, &acked);
    println!(
        "acked {} keys, {} sstable(s) on disk, all recovered",
        acked.len(),
        sstable_count(&dir.0)
    );
}
