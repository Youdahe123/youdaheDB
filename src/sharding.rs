// Sharding. Hashes each key to one of N shards, each with its own lsm.rs
// storage engine, so the keyspace splits across machines and shards can be
// written in parallel. The hash has to stay the same across restarts.
// Empty for now.
