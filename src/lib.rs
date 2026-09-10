#![allow(non_snake_case)]
//! A persistent key-value engine that can be shared between threads.
//!
//! Open a data directory once, then clone [`Engine`] to share that instance.
//!
//! ```no_run
//! use youdaheDB::Engine;
//!
//! # fn main() -> std::io::Result<()> {
//! let db = Engine::open("data")?;
//! let writer = db.clone();
//! std::thread::spawn(move || writer.put("user:1", "alice"))
//!     .join().expect("writer panicked")?;
//! assert_eq!(db.get("user:1")?, Some("alice".to_string()));
//! # Ok(())
//! # }
//! ```

mod engine;
mod hash_ring;
mod lsm;
mod memtable;
mod merge;
mod sstable;
mod wal;

pub use engine::{Engine, Scan};
pub use hash_ring::{HashRing, RingError};
