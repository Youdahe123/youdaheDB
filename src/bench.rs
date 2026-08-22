// TODO: drop this allow once the remaining todo!()s are implemented
#![allow(unused_variables, dead_code)]

use std::time::{Duration, Instant};

// Head-to-head throughput/latency benchmark against a real Redis instance.
// Both sides are driven through the same redis client over the same protocol,
// so the only difference measured is the server.

pub struct BenchConfig {
    pub ops: usize,
    pub value_size: usize,
    pub rustdb_addr: String,
    pub redis_addr: String,
    // share of operations that are GETs, e.g. 0.5 = an even read/write mix
    pub read_ratio: f64,
}

impl Default for BenchConfig {
    fn default() -> BenchConfig {
        BenchConfig {
            ops: 20_000,
            value_size: 100,
            rustdb_addr: "127.0.0.1:7878".to_string(),
            redis_addr: "127.0.0.1:6379".to_string(),
            read_ratio: 0.5,
        }
    }
}

pub struct BenchResult {
    pub label: String,
    pub ops: usize,
    pub elapsed: Duration,
    pub ops_per_sec: f64,
    pub p50_micros: u64,
    pub p99_micros: u64,
}

impl BenchResult {
    // percentiles, not just the mean — an average hides the tail latency that
    // actually shows up as a slow request
    pub fn from_latencies(label: &str, mut latencies: Vec<Duration>, elapsed: Duration) -> BenchResult {
        latencies.sort();
        let ops = latencies.len();
        let at = |percentile: usize| -> u64 {
            if ops == 0 {
                return 0;
            }
            let index = (ops * percentile / 100).min(ops - 1);
            latencies[index].as_micros() as u64
        };

        BenchResult {
            label: label.to_string(),
            ops,
            elapsed,
            ops_per_sec: ops as f64 / elapsed.as_secs_f64().max(f64::EPSILON),
            p50_micros: at(50),
            p99_micros: at(99),
        }
    }

    pub fn print_row(&self) {
        println!(
            "{:<10} {:>10} {:>14.0} {:>12} {:>12}",
            self.label, self.ops, self.ops_per_sec, self.p50_micros, self.p99_micros
        );
    }
}

// deterministic payload of exactly value_size bytes, so both runs push
// identical data and the comparison stays apples-to-apples
fn make_value(value_size: usize) -> String {
    "x".repeat(value_size)
}

// drives `ops` requests against one address, timing each individually
fn run_against(label: &str, addr: &str, config: &BenchConfig) -> Result<BenchResult, redis::RedisError> {
    let client = redis::Client::open(format!("redis://{addr}"))?;
    let mut conn = client.get_connection()?;

    let value = make_value(config.value_size);
    let mut latencies = Vec::with_capacity(config.ops);
    let start = Instant::now();

    for i in 0..config.ops {
        // keys cycle over a bounded set so GETs actually hit existing data
        let key = format!("key:{}", i % 10_000);

        // deterministic read/write mix — no rng, so runs are reproducible
        let is_read = (i % 100) < (config.read_ratio * 100.0) as usize;

        let op_start = Instant::now();
        if is_read {
            redis::cmd("GET").arg(&key).query::<Option<String>>(&mut conn)?;
        } else {
            redis::cmd("SET").arg(&key).arg(&value).query::<()>(&mut conn)?;
        }
        latencies.push(op_start.elapsed());
    }

    Ok(BenchResult::from_latencies(label, latencies, start.elapsed()))
}

pub fn bench_rustdb(config: &BenchConfig) -> Result<BenchResult, redis::RedisError> {
    run_against("rustdb", &config.rustdb_addr, config)
}

pub fn bench_redis(config: &BenchConfig) -> Result<BenchResult, redis::RedisError> {
    run_against("redis", &config.redis_addr, config)
}

// DESIGN: what makes this benchmark honest is the caveats, not the numbers.
// Both servers are warmed the same way, run the same mix, and are measured
// from the same client — and the write-up has to say what rustdb ISN'T doing
// that redis is (no expiry, no eviction, no persistence tuning, single
// connection, loopback only).
pub fn run_comparison(config: &BenchConfig) -> Result<(), redis::RedisError> {
    println!(
        "{:<10} {:>10} {:>14} {:>12} {:>12}",
        "target", "ops", "ops/sec", "p50 (us)", "p99 (us)"
    );

    todo!("run bench_rustdb and bench_redis, print_row() each, then print the \
           caveats that make the comparison fair")
}
