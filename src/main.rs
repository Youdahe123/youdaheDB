// TODO: drop this allow once run_demo() is implemented
#![allow(unused_variables, dead_code)]

use rustdb::bench::{run_comparison, BenchConfig};
use rustdb::consistency::ConsistencyLevel;
use rustdb::server::{Server, ServerConfig};

// CLI entry point. Three subcommands:
//   rustdb demo
//   rustdb server --port 7878 --shards 4 --consistency strong
//   rustdb bench  --ops 20000 --value-size 100

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("demo") => run_demo(),
        Some("server") => run_server(&args[1..]),
        Some("bench") => run_bench(&args[1..]),
        _ => print_usage(),
    }
}

// DESIGN: this is the screenshot. It should walk every layer in order and
// narrate what each one did — WAL survives a simulated crash, the LSM read
// path finds the newest version, the hash spreads keys evenly over shards,
// raft refuses to commit without a quorum, an EVENTUAL read visibly returns a
// staler value than a STRONG one, and two CRDT replicas converge after
// merging. Pick an order that tells a story rather than one call per module.
fn run_demo() {
    todo!("walk the layers in order, printing what each one proves")
}

fn run_server(args: &[String]) {
    let config = parse_server_args(args);

    match Server::new(config).and_then(|mut server| server.run()) {
        Ok(()) => {}
        Err(err) => eprintln!("server failed: {err}"),
    }
}

fn run_bench(args: &[String]) {
    let config = parse_bench_args(args);

    if let Err(err) = run_comparison(&config) {
        eprintln!("benchmark failed: {err}");
        eprintln!("is redis-server running, and is rustdb up on the bench port?");
    }
}

// unrecognised or malformed flags fall back to the default rather than
// aborting, so a typo doesn't kill a long benchmark run
fn parse_server_args(args: &[String]) -> ServerConfig {
    let defaults = ServerConfig::default();

    ServerConfig {
        port: flag(args, "--port").and_then(|v| v.parse().ok()).unwrap_or(defaults.port),
        num_shards: flag(args, "--shards").and_then(|v| v.parse().ok()).unwrap_or(defaults.num_shards),
        consistency: flag(args, "--consistency")
            .and_then(ConsistencyLevel::from_str)
            .unwrap_or(defaults.consistency),
        data_dir: flag(args, "--data-dir").map(|v| v.to_string()).unwrap_or(defaults.data_dir),
    }
}

fn parse_bench_args(args: &[String]) -> BenchConfig {
    let defaults = BenchConfig::default();

    BenchConfig {
        ops: flag(args, "--ops").and_then(|v| v.parse().ok()).unwrap_or(defaults.ops),
        value_size: flag(args, "--value-size").and_then(|v| v.parse().ok()).unwrap_or(defaults.value_size),
        rustdb_addr: flag(args, "--rustdb").map(|v| v.to_string()).unwrap_or(defaults.rustdb_addr),
        redis_addr: flag(args, "--redis").map(|v| v.to_string()).unwrap_or(defaults.redis_addr),
        read_ratio: flag(args, "--read-ratio").and_then(|v| v.parse().ok()).unwrap_or(defaults.read_ratio),
    }
}

// pulls "--name value" out of the arg list, returning None if the flag is
// absent or has nothing after it
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|i| args.get(i + 1))
        .map(|value| value.as_str())
}

fn print_usage() {
    println!("rustdb — a small distributed KV store\n");
    println!("USAGE:");
    println!("    rustdb demo");
    println!("    rustdb server [--port 7878] [--shards 4] [--consistency strong|eventual] [--data-dir ./data]");
    println!("    rustdb bench  [--ops 20000] [--value-size 100] [--read-ratio 0.5]");
}
