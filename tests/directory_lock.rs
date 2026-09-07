use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use youdaheDB::Engine;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn another_process_is_excluded_and_process_death_releases_ownership() {
    let root = std::env::temp_dir().join(format!("youdahedb-process-lock-{}", std::process::id()));
    // Never reuse or remove an existing directory from a previous run.
    let mut suffix = 0;
    let dir = loop {
        let path = root.with_extension(suffix.to_string());
        match fs::create_dir(&path) {
            Ok(()) => break TestDir(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => suffix += 1,
            Err(e) => panic!("create test directory: {e}"),
        }
    };
    let mut owner = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_youdaheDB"))
            .current_dir(&dir.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = owner.0.stdout.take().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let output = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        ready_tx.send(line).unwrap();
        // Keep the pipe open while the REPL prints its remaining startup text.
        io::copy(&mut reader, &mut io::sink()).unwrap();
    });
    assert!(ready_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .contains("youdaheDB"));
    let data = dir.0.join("data");
    let error = Engine::open(data.to_str().unwrap()).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    owner.0.kill().unwrap(); // SIGKILL on Unix: no destructor-based unlock
    owner.0.wait().unwrap();
    output.join().unwrap();
    let db = Engine::open(data.to_str().unwrap()).unwrap();
    db.put("k", "v").unwrap();
    let competing = Command::new(env!("CARGO_BIN_EXE_youdaheDB"))
        .current_dir(&dir.0)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!competing.status.success());
    assert!(String::from_utf8_lossy(&competing.stderr).contains("already open"));
    assert_eq!(db.get("k").unwrap(), Some("v".into()));
}
