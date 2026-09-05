mod wal;
mod memtable;
mod sstable;
mod merge;

use wal::Wal;
use memtable::{Lookup, MemTable};
use std::io::{self, BufRead};

// replay the wal to rebuild memtable state after a crash
fn recover(wal_path: &str) -> io::Result<MemTable> {
    let mut memtable = MemTable::new();
    let entries = Wal::replay(wal_path)?;
    let count = entries.len();

    for entry in entries {
        match entry.value {
            Some(v) => memtable.put(entry.key, v),
            None => memtable.delete(&entry.key),
        }
    }

    if count > 0 {
        println!("recovered {} entries from WAL", count);
    }

    Ok(memtable)
}

fn main() -> io::Result<()> {
    let wal_path = "data.wal";

    let mut memtable = recover(wal_path)?;
    let mut wal = Wal::open(wal_path)?;

    println!("youdaheDB v0.1");
    println!("commands: put <key> <value> | get <key> | delete <key> | scan | quit");
    println!();

    let stdin = io::stdin();
    loop {
        eprint!("db> ");
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();

        match parts[0] {
            "put" => {
                if parts.len() < 3 {
                    println!("usage: put <key> <value>");
                    continue;
                }
                // write to wal first for durability, then memtable for speed
                wal.put(parts[1], parts[2])?;
                memtable.put(parts[1].to_string(), parts[2].to_string());
                println!("OK");
            }

            "get" => {
                if parts.len() < 2 {
                    println!("usage: get <key>");
                    continue;
                }
                match memtable.get(parts[1]) {
                    Lookup::Found(value) => println!("{}", value),
                    Lookup::Deleted => println!("(deleted)"),
                    Lookup::NotFound => println!("(not found)"),
                }
            }

            "delete" => {
                if parts.len() < 2 {
                    println!("usage: delete <key>");
                    continue;
                }
                wal.delete(parts[1])?;
                memtable.delete(parts[1]);
                println!("OK");
            }

            "scan" => {
                let mut count = 0;
                for (key, value) in memtable.iter() {
                    match value {
                        Some(v) => println!("  {} = {}", key, v),
                        None => println!("  {} (deleted)", key),
                    }
                    count += 1;
                }
                if count == 0 {
                    println!("(empty)");
                }
            }

            "quit" | "exit" => {
                println!("bye");
                break;
            }

            _ => {
                println!("unknown command: {}", parts[0]);
            }
        }
    }

    Ok(())
}
