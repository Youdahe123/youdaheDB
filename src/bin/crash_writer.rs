

use std::io;
use youdaheDB::Engine;

// 16 KiB fills the 4 MiB memtable every 256 writes, so flushes happen about
// once a second. Smaller values never reach a flush before the kill lands.
const DEFAULT_VALUE_BYTES: usize = 16384;

// Derived from the index so the checker can spot a wrong value, not just a
// missing one. ASCII only, so slicing by byte length is safe.
fn value_for(i: u64, size: usize) -> String {
    let seed = format!("{i}:");
    seed.repeat(size / seed.len() + 1)[..size].to_string()
}

fn usage(message: &str) -> ! {
    eprintln!("{message}\nusage: crash_writer <data-dir> [value-bytes]");
    std::process::exit(2);
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        usage("missing data directory");
    };
    let value_bytes = match args.next() {
        None => DEFAULT_VALUE_BYTES,
        Some(raw) => match raw.parse() {
            Ok(n) if n > 0 => n,
            _ => usage("value-bytes must be a positive integer"),
        },
    };

    let db = Engine::open(&dir)?;

    // Exiting non-zero on error lets the parent tell a crash from a kill.
    for i in 0u64.. {
        db.put(&format!("key:{i}"), &value_for(i, value_bytes))?;
        // Announce only after put returns. println! flushes on the newline;
        // a BufWriter here would strand acks when SIGKILL lands.
        println!("key:{i}");
    }

    Ok(())
}
