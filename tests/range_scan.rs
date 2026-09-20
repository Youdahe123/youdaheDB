use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use youdaheDB::{Engine, Scan};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "youdahedb-range-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        }
    }
    fn open(&self) -> Engine {
        Engine::open(self.0.to_str().unwrap()).unwrap()
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rows(scan: Scan<'_>) -> Vec<(String, String)> {
    scan.iter().unwrap().collect::<io::Result<_>>().unwrap()
}
const KEYS: &[&str] = &[
    "",
    "a",
    "b",
    "user",
    "user:",
    "user:1",
    "user:2",
    "user;",
    "é",
    "é:1",
    "\u{10ffff}",
    "\u{10ffff}x",
];

fn check(db: &Engine, model: &BTreeMap<String, String>, context: &str) {
    let bounds = [
        None,
        Some(""),
        Some("a"),
        Some("aa"),
        Some("user:"),
        Some("user:1"),
        Some("user;"),
        Some("é"),
        Some("\u{10ffff}"),
        Some("\u{10ffff}z"),
    ];
    for start in bounds {
        for end in bounds {
            let expected: Vec<_> = model
                .iter()
                .filter(|(k, _)| {
                    start.is_none_or(|s| k.as_str() >= s) && end.is_none_or(|e| k.as_str() < e)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            assert_eq!(
                rows(db.scan_range(start, end).unwrap()),
                expected,
                "{context}: range {start:?}..{end:?}"
            );
        }
    }
    for prefix in KEYS.iter().copied().chain(["missing", "e\u{301}"]) {
        let expected: Vec<_> = model
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(
            rows(db.scan_prefix(prefix).unwrap()),
            expected,
            "{context}: prefix {prefix:?}"
        );
    }
    let expected: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(rows(db.scan().unwrap()), expected, "{context}: full scan");
}

#[test]
fn boundaries_versions_tombstones_and_reopen() {
    let dir = TestDir::new();
    let db = dir.open();
    let mut model = BTreeMap::new();
    check(&db, &model, "empty");
    for key in KEYS {
        db.put(key, "old").unwrap();
        model.insert(key.to_string(), "old".into());
    }
    db.flush().unwrap();
    db.delete("user:1").unwrap();
    model.remove("user:1");
    db.put("user:2", "middle").unwrap();
    model.insert("user:2".into(), "middle".into());
    db.flush().unwrap();
    db.delete("user:2").unwrap();
    model.remove("user:2");
    db.put("user:1", "revived").unwrap();
    model.insert("user:1".into(), "revived".into());
    db.put("a", "new").unwrap();
    model.insert("a".into(), "new".into());
    check(&db, &model, "two SSTables plus memtable");
    drop(db);
    let db = dir.open();
    check(&db, &model, "WAL reopen");
    db.flush().unwrap();
    check(&db, &model, "flushed");
    drop(db);
    check(&dir.open(), &model, "SSTable reopen");
}

#[test]
fn bounds_are_owned_and_guards_can_be_iterated_again() {
    let dir = TestDir::new();
    let db = dir.open();
    db.put("user:1", "v").unwrap();
    for scan in [
        {
            let start = String::from("user:");
            let end = String::from("user;");
            db.scan_range(Some(&start), Some(&end)).unwrap()
        },
        {
            let prefix = String::from("user:");
            db.scan_prefix(&prefix).unwrap()
        },
    ] {
        for _ in 0..2 {
            assert_eq!(
                scan.iter()
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap(),
                vec![("user:1".into(), "v".into())]
            );
        }
    }
}

#[test]
fn seeded_mutations_match_independent_model() {
    for seed in 1..=8u64 {
        let dir = TestDir::new();
        let mut db = dir.open();
        let mut state = seed;
        let mut model = BTreeMap::new();
        let mut trace = Vec::new();
        for step in 0..64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let key = KEYS[((state >> 32) as usize) % KEYS.len()];
            if state >> 62 == 0 {
                db.delete(key).unwrap();
                model.remove(key);
                trace.push(format!("delete {key:?}"));
            } else {
                let value = format!("{seed}:{step}");
                db.put(key, &value).unwrap();
                model.insert(key.into(), value.clone());
                trace.push(format!("put {key:?} {value:?}"));
            }
            if step % 16 == 15 {
                let context = format!("seed={seed} trace={trace:?}");
                check(&db, &model, &context);
                db.flush().unwrap();
                check(&db, &model, &context);
                drop(db);
                db = dir.open();
                check(&db, &model, &context);
            }
        }
    }
}
