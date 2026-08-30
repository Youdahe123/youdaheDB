// Crate root. The stack, bottom-up — each layer depends only on the ones below:
//
//   server.rs       RESP/TCP front door
//   consistency.rs  strong (leader) vs eventual (follower) reads
//   sharding.rs     splits the keyspace across N engines by hash
//   raft.rs         replicates the write log, commits on a majority
//   lsm.rs          storage engine — the only live layer (WAL only)
//
//   crdt.rs         off to the side: merge instead of coordinate
//   bench.rs        off to the side: rustdb vs redis
//
// Everything but lsm.rs is a placeholder, built one at a time from the bottom.

pub mod lsm;
pub mod raft;
pub mod sharding;
pub mod crdt;
pub mod consistency;
pub mod server;
pub mod bench;
