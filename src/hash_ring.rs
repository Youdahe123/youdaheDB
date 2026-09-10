//! Deterministic key ownership. This module calculates placement; it does not
//! store data, send requests, or migrate keys when membership changes.

use std::collections::BTreeSet;

mod error;
mod hash;

pub use error::RingError;
use hash::{key_hash, node_hash};

const DEFAULT_VIRTUAL_NODES: u32 = 256;

// Ordering is part of the placement contract: collisions are resolved by node
// ID (UTF-8 byte order), then virtual index, independently of insertion order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    hash: u64,
    node_id: String,
    virtual_index: u32,
}

/// A consistent hash ring with equal-weight physical nodes.
///
/// Identical node IDs and virtual-node counts yield identical ownership across
/// processes. Changing IDs, the count, or the hashing rules changes placement.
/// An empty ring has no owner. Membership changes only update this mapping;
/// callers are responsible for migrating any stored data. Cloning creates an
/// independent copy; later membership changes are not shared between clones.
///
/// ```
/// use youdaheDB::HashRing;
/// let mut ring = HashRing::default(); // 256 virtual positions per node
/// assert!(ring.add_node("node-a"));
/// assert_eq!(ring.owner("user:123"), Some("node-a"));
/// assert!(ring.remove_node("node-a"));
/// assert_eq!(ring.owner("user:123"), None);
/// ```
#[derive(Debug, Clone)]
pub struct HashRing {
    virtual_nodes: u32,
    nodes: BTreeSet<String>,
    positions: Vec<Position>,
}

impl Default for HashRing {
    fn default() -> Self {
        Self::new(DEFAULT_VIRTUAL_NODES).expect("default virtual-node count is positive")
    }
}

impl HashRing {
    /// Creates an empty ring with the given number of virtual nodes per node.
    ///
    /// Each node allocates this many positions when added. There is no upper
    /// resource limit beyond the `u32` count; callers should choose a count
    /// appropriate for their memory budget and expected membership size.
    ///
    /// # Errors
    ///
    /// Returns [`RingError::ZeroVirtualNodes`] if `virtual_nodes` is zero.
    pub fn new(virtual_nodes: u32) -> Result<Self, RingError> {
        if virtual_nodes == 0 {
            return Err(RingError::ZeroVirtualNodes);
        }
        Ok(Self {
            virtual_nodes,
            nodes: BTreeSet::new(),
            positions: Vec::new(),
        })
    }

    /// Adds a stable node ID. Returns false if it was already present.
    /// IDs are arbitrary UTF-8 strings, including the empty string.
    pub fn add_node(&mut self, node_id: &str) -> bool {
        if !self.nodes.insert(node_id.to_owned()) {
            return false;
        }
        for virtual_index in 0..self.virtual_nodes {
            self.positions.push(Position {
                hash: node_hash(node_id, virtual_index),
                node_id: node_id.to_owned(),
                virtual_index,
            });
        }
        self.positions.sort_unstable();
        true
    }

    /// Removes a node and all its positions. Returns false if it was absent.
    pub fn remove_node(&mut self, node_id: &str) -> bool {
        if !self.nodes.remove(node_id) {
            return false;
        }
        self.positions
            .retain(|position| position.node_id != node_id);
        true
    }

    /// Returns the first owner clockwise from the key's hash, wrapping around.
    /// Lookup takes O(log V) comparisons for V total virtual positions and
    /// neither allocates nor performs I/O. The result borrows from this ring.
    pub fn owner(&self, key: &str) -> Option<&str> {
        self.owner_at(key_hash(key))
    }

    fn owner_at(&self, hash: u64) -> Option<&str> {
        let index = self
            .positions
            .partition_point(|position| position.hash < hash);
        self.positions
            .get(index)
            .or_else(|| self.positions.first())
            .map(|position| position.node_id.as_str())
    }
}

#[cfg(test)]
mod tests;
