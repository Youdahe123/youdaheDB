// TODO: drop this allow once the stubs below are implemented
#![allow(unused_variables, dead_code)]

use crate::lsm::{stable_hash, LsmTree};

// Routes keys across N independent shards by hash, so each shard owns a
// disjoint slice of the keyspace and they can be written in parallel.

pub struct ShardRouter {
    num_shards: usize,
}

impl ShardRouter {
    pub fn new(num_shards: usize) -> ShardRouter {
        assert!(num_shards > 0, "a store needs at least one shard");
        ShardRouter { num_shards }
    }

    // must not change between runs, or every key would relocate on restart
    pub fn hash_key(key: &str) -> u64 {
        stable_hash(key, 0)
    }

    // DESIGN NOTE: modulo routing is simple but rehashes ~all keys when
    // num_shards changes. Consistent hashing (a ring of virtual nodes) moves
    // only ~1/N of them — that's what `adding_node_remaps_minimal_keys` in
    // tests/integration.rs is there to prove, and swapping this one method
    // is all it takes.
    pub fn shard_for_key(&self, key: &str) -> usize {
        (Self::hash_key(key) % self.num_shards as u64) as usize
    }

    pub fn num_shards(&self) -> usize {
        self.num_shards
    }
}

// the fan-out layer the server talks to: one storage engine per shard
pub struct ShardedStore {
    router: ShardRouter,
    shards: Vec<LsmTree>,
}

impl ShardedStore {
    // one storage engine per shard, each in its own subdirectory so the shards
    // share no files and can be written concurrently
    pub fn open(dir: &str, num_shards: usize) -> Result<ShardedStore, std::io::Error> {
        let shards = (0..num_shards)
            .map(|i| LsmTree::open(&format!("{dir}/shard-{i}")))
            .collect::<Result<Vec<LsmTree>, std::io::Error>>()?;

        Ok(ShardedStore { router: ShardRouter::new(num_shards), shards })
    }

    pub fn put(&mut self, key: &str, value: &str) -> Result<(), std::io::Error> {
        let shard = self.router.shard_for_key(key);
        self.shards[shard].put(key, value)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, std::io::Error> {
        let shard = self.router.shard_for_key(key);
        self.shards[shard].get(key)
    }

    pub fn delete(&mut self, key: &str) -> Result<(), std::io::Error> {
        let shard = self.router.shard_for_key(key);
        self.shards[shard].delete(key)
    }

    pub fn num_shards(&self) -> usize {
        self.router.num_shards()
    }

    // per-shard key counts — the demo uses this to show the spread is even
    pub fn shard_stats(&self) -> Vec<usize> {
        todo!("needs a key-count method on LsmTree first (memtable.len() plus \
               the sstable entries, minus tombstones)")
    }
}
