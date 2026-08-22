// TODO: drop this allow once the stubs below are implemented
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;

// Conflict-free replicated data types: replicas accept writes independently
// and still converge, because merge() is commutative, associative, idempotent.

// anything that can absorb another replica's state without coordination
pub trait Crdt {
    fn merge(&mut self, other: &Self);
}

// grow-only counter: each node owns its own slot and only ever increases it,
// so merging is just taking the max per slot — no lost updates possible
pub struct GCounter {
    counts: HashMap<String, u64>,
}

impl GCounter {
    pub fn new() -> GCounter {
        GCounter { counts: HashMap::new() }
    }

    // bumps only this node's slot — no other node's entry is ever touched,
    // which is exactly why two replicas can increment concurrently without
    // coordinating
    pub fn increment(&mut self, node_id: &str, amount: u64) {
        *self.counts.entry(node_id.to_string()).or_insert(0) += amount;
    }

    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl Crdt for GCounter {
    // DESIGN: why per-slot MAX and not sum? Summing would double-count any
    // increment that already reached this replica by another path, so merge
    // would stop being idempotent and gossip would inflate the counter.
    fn merge(&mut self, other: &GCounter) {
        todo!("for each (node_id, count) in other.counts, keep the larger of \
               the two values")
    }
}

// positive-negative counter: two G-Counters, since decrements can't be
// expressed in a grow-only structure. value = increments - decrements.
pub struct PnCounter {
    positive: GCounter,
    negative: GCounter,
}

impl PnCounter {
    pub fn new() -> PnCounter {
        PnCounter { positive: GCounter::new(), negative: GCounter::new() }
    }

    pub fn increment(&mut self, node_id: &str, amount: u64) {
        self.positive.increment(node_id, amount);
    }

    // recorded as growth in the negative counter, never as a subtraction —
    // subtracting would break the grow-only property that makes merge safe
    pub fn decrement(&mut self, node_id: &str, amount: u64) {
        self.negative.increment(node_id, amount);
    }

    // i64 because decrements can push the result below zero
    pub fn value(&self) -> i64 {
        self.positive.value() as i64 - self.negative.value() as i64
    }
}

impl Crdt for PnCounter {
    // both halves are G-Counters, so this is just their merge twice over
    fn merge(&mut self, other: &PnCounter) {
        self.positive.merge(&other.positive);
        self.negative.merge(&other.negative);
    }
}

// last-writer-wins register: the highest timestamp wins, node_id breaks ties
// deterministically so every replica picks the same winner
pub struct LwwRegister {
    value: String,
    timestamp: u64,
    node_id: String,
}

impl LwwRegister {
    pub fn new(node_id: &str) -> LwwRegister {
        LwwRegister { value: String::new(), timestamp: 0, node_id: node_id.to_string() }
    }

    pub fn set(&mut self, value: &str, timestamp: u64) {
        self.value = value.to_string();
        self.timestamp = timestamp;
    }

    pub fn get(&self) -> &str {
        &self.value
    }
}

impl Crdt for LwwRegister {
    // DESIGN: the tie-break is the whole trick. Two replicas can legitimately
    // stamp the same timestamp (clocks are not that precise), and if the tie
    // resolves differently on each side they diverge forever. Comparing
    // node_id on a tie is arbitrary but *deterministic*, which is all
    // convergence requires. Note this data type genuinely loses a write —
    // that's the cost of LWW, not a bug in the implementation.
    fn merge(&mut self, other: &LwwRegister) {
        todo!("take other's value if its timestamp is higher, or if the \
               timestamps are equal and its node_id sorts higher")
    }
}
