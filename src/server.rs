// TODO: drop this allow once the remaining todo!()s are implemented
#![allow(unused_variables, dead_code)]

use crate::consistency::ConsistencyLevel;
use crate::sharding::ShardedStore;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

// A RESP-speaking TCP server, so `redis-cli` and the redis crate can both talk
// to it unmodified — which is what makes the head-to-head benchmark fair.

pub struct ServerConfig {
    pub port: u16,
    pub num_shards: usize,
    pub consistency: ConsistencyLevel,
    pub data_dir: String,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            port: 7878,
            num_shards: 4,
            consistency: ConsistencyLevel::Strong,
            data_dir: "./data".to_string(),
        }
    }
}

// the subset of the redis command set we implement
pub enum Command {
    Get { key: String },
    Set { key: String, value: String },
    Del { key: String },
    Ping,
    Info,
    Unknown(String),
}

// what a handler produces, before it's encoded back into RESP bytes
pub enum Reply {
    Ok,
    Bulk(String),
    Nil,
    Integer(i64),
    Error(String),
}

pub struct Server {
    config: ServerConfig,
    // one store shared by every connection thread. DESIGN NOTE: a single Mutex
    // serializes all shards, which throws away the parallelism sharding just
    // bought you. Moving the lock inside ShardedStore (one per shard) is the
    // obvious next step once the benchmark is running.
    store: Arc<Mutex<ShardedStore>>,
}

impl Server {
    pub fn new(config: ServerConfig) -> Result<Server, std::io::Error> {
        let store = ShardedStore::open(&config.data_dir, config.num_shards)?;
        Ok(Server { config, store: Arc::new(Mutex::new(store)) })
    }

    // binds the port and accepts connections forever, one thread per client
    pub fn run(&mut self) -> Result<(), std::io::Error> {
        let listener = TcpListener::bind(("127.0.0.1", self.config.port))?;
        println!(
            "rustdb listening on 127.0.0.1:{} — {} shards, {} consistency",
            self.config.port,
            self.config.num_shards,
            self.config.consistency.as_str()
        );

        for stream in listener.incoming() {
            let stream = stream?;
            let store = Arc::clone(&self.store);
            let level = self.config.consistency;

            std::thread::spawn(move || {
                if let Err(err) = handle_client(stream, store, level) {
                    eprintln!("client error: {err}");
                }
            });
        }

        Ok(())
    }
}

// reads commands off one connection until the client disconnects
fn handle_client(
    stream: TcpStream,
    store: Arc<Mutex<ShardedStore>>,
    level: ConsistencyLevel,
) -> Result<(), std::io::Error> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    while let Some(command) = parse_command(&mut reader)? {
        let reply = handle_command(command, &store, level);
        writer.write_all(&encode_reply(&reply))?;
        writer.flush()?;
    }

    Ok(())
}

// DESIGN: this is where consistency level stops being a config string and
// starts changing behaviour — a Get under Eventual should be allowed to serve
// a stale replica, a Set always goes to the leader. Decide too what a Set
// returns before it is fully replicated: acking early is fast but can lie.
fn handle_command(
    command: Command,
    store: &Arc<Mutex<ShardedStore>>,
    level: ConsistencyLevel,
) -> Reply {
    todo!("match the command, take the store lock, call put/get/delete, and map \
           io errors onto Reply::Error")
}

// ─── RESP wire protocol ──────────────────────────────────────────────────────
// clients send an array of bulk strings: "*3\r\n$3\r\nSET\r\n$1\r\na\r\n$1\r\nb\r\n"
// Ok(None) means the client closed the connection cleanly.
fn parse_command<R: BufRead>(reader: &mut R) -> Result<Option<Command>, std::io::Error> {
    let header = match read_line(reader)? {
        Some(line) => line,
        None => return Ok(None),
    };

    // inline commands (what you get from `nc`/telnet rather than redis-cli)
    // aren't length-prefixed, so split them on whitespace instead
    if !header.starts_with('*') {
        let parts: Vec<String> = header.split_whitespace().map(|s| s.to_string()).collect();
        return Ok(Some(build_command(parts)));
    }

    let count: usize = header[1..].trim().parse().unwrap_or(0);
    let mut parts = Vec::with_capacity(count);

    for _ in 0..count {
        // each argument is preceded by its own "$<len>" header
        let len_line = match read_line(reader)? {
            Some(line) => line,
            None => return Ok(None),
        };
        let len: usize = len_line[1..].trim().parse().unwrap_or(0);

        // read exactly len bytes, then discard the trailing \r\n
        let mut buf = vec![0u8; len + 2];
        reader.read_exact(&mut buf)?;
        buf.truncate(len);
        parts.push(String::from_utf8_lossy(&buf).into_owned());
    }

    Ok(Some(build_command(parts)))
}

// reads one \r\n-terminated line. Ok(None) at end of stream.
fn read_line<R: BufRead>(reader: &mut R) -> Result<Option<String>, std::io::Error> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

// command names are case-insensitive in redis
fn build_command(parts: Vec<String>) -> Command {
    let name = parts.first().map(|s| s.to_ascii_uppercase()).unwrap_or_default();

    match (name.as_str(), parts.len()) {
        ("GET", 2) => Command::Get { key: parts[1].clone() },
        ("SET", 3) => Command::Set { key: parts[1].clone(), value: parts[2].clone() },
        ("DEL", 2) => Command::Del { key: parts[1].clone() },
        ("PING", _) => Command::Ping,
        ("INFO", _) => Command::Info,
        _ => Command::Unknown(name),
    }
}

fn encode_reply(reply: &Reply) -> Vec<u8> {
    match reply {
        Reply::Ok => b"+OK\r\n".to_vec(),
        // bulk strings are length-prefixed so values may contain \r\n safely
        Reply::Bulk(value) => format!("${}\r\n{}\r\n", value.len(), value).into_bytes(),
        // the null bulk string — distinct from an empty one, and how redis says
        // "no such key"
        Reply::Nil => b"$-1\r\n".to_vec(),
        Reply::Integer(n) => format!(":{n}\r\n").into_bytes(),
        Reply::Error(message) => format!("-ERR {message}\r\n").into_bytes(),
    }
}
