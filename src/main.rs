use std::io::{self, BufRead};
use youdaheDB::Engine;

const DATA_DIR: &str = "data";

fn main() -> io::Result<()> {
    // open replays the wal and loads existing sstables
    let db = Engine::open(DATA_DIR)?;

    println!("youdaheDB v0.1");
    println!("commands: put <key> <value> | get <key> | delete <key> | scan | flush | quit");
    println!();

    let stdin = io::stdin();
    loop {
        eprint!("db> ");
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // stdin closed
        }
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
                db.put(parts[1], parts[2])?;
                println!("OK");
            }

            "get" => {
                if parts.len() < 2 {
                    println!("usage: get <key>");
                    continue;
                }
                match db.get(parts[1])? {
                    Some(value) => println!("{}", value),
                    None => println!("(not found)"),
                }
            }

            "delete" => {
                if parts.len() < 2 {
                    println!("usage: delete <key>");
                    continue;
                }
                db.delete(parts[1])?;
                println!("OK");
            }

            "scan" => {
                let mut count = 0;
                let scan = db.scan()?;
                for entry in scan.iter()? {
                    let (key, value) = entry?;
                    println!("  {} = {}", key, value);
                    count += 1;
                }
                if count == 0 {
                    println!("(empty)");
                }
            }

            "flush" => {
                db.flush()?;
                println!("OK · {} sstable(s)", db.sstable_count()?);
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
