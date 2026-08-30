// Binary entry point. Parses argv and dispatches to `demo`, `server`, or
// `bench` — no database logic of its own. Empty for now.

fn main() {
    println!("rustdb — a small distributed KV store");
    println!("nothing wired up yet; the WAL in src/lsm.rs is the only live layer.");
    println!("run `cargo test` to exercise it.");
}
