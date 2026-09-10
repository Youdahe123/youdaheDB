# Consistent hash ring

`HashRing` calculates which physical node owns a key. It keeps virtual node
positions in memory; values remain in the owning node's storage engine. It
performs no networking or data migration. Every participant must use the same
membership, stable node IDs, virtual-node count, and placement algorithm.

```rust
use youdaheDB::HashRing;

fn main() -> Result<(), youdaheDB::RingError> {
    let mut ring = HashRing::new(256)?;
    ring.add_node("node-a");
    ring.add_node("node-b");
    if let Some(node) = ring.owner("user:123") {
        println!("Owner: {node}");
    }
    ring.remove_node("node-b");
    Ok(())
}
```

`Default` uses 256 virtual positions per physical node. `new(0)` returns
`RingError::ZeroVirtualNodes`. Node IDs and keys accept arbitrary UTF-8 strings,
including empty strings. Adding an existing node or removing an absent node
returns false and leaves the mapping unchanged. Successful changes return true.
An empty ring returns `None` from `owner`; otherwise the returned `&str` borrows
the node ID from the ring. Mutation requires exclusive access via `&mut self`.
Cloning creates an independent copy, so membership updates must be propagated
explicitly; clones do not share updates like `Engine` handles do.

## Placement v1 compatibility contract

Hashes are unsigned 64-bit values. Hash the following exact byte sequences:

- Key: byte `0`, followed by the UTF-8 key bytes.
- Virtual position: byte `1`, UTF-8 node ID bytes, then a four-byte unsigned
  virtual index in little-endian order, from zero to the configured count minus one.

The fixed-width index avoids ambiguous concatenations. The leading byte keeps
key inputs separate from virtual-node inputs. There is no Unicode normalization.

Apply FNV-1a with initial value `0xcbf29ce484222325`: for each byte, XOR it into
that value and multiply by `0x100000001b3`. Then apply these finalization steps:

```text
h ^= h >> 33
h *= 0xff51afd7ed558ccd
h ^= h >> 33
h *= 0xc4ceb9fe1a85ec53
h ^= h >> 33
```

All multiplication wraps modulo 2^64. This fixed, non-cryptographic algorithm
uses no randomized process seed or unspecified standard-library hasher.

Sort positions by `(hash, node ID, virtual index)`, with node IDs compared in
UTF-8 byte order. Retain colliding positions. A key belongs to the first position
whose hash is at least its hash, wrapping to the first position when needed.
Thus the smallest node ID wins a position collision until removed.

These rules are a compatibility contract: changing hashing, byte encoding,
ordering, IDs, or virtual-node counts can remap keys and requires coordinated
migration by the caller. No ring format is persisted by this module.

## Cost

For V total virtual positions, lookup takes O(log V) comparisons plus hashing
the key, with no allocation or I/O. Adding a node sorts the positions in
O(V log V) time. Removing a node scans positions in O(V) time. Memory grows with
V and the node-ID lengths; each position owns a copy of its node ID. This simple
representation favors frequent lookups and infrequent membership changes.
The constructor rejects zero but sets no upper resource limit: a very large
virtual-node count can exhaust memory when a node is added. Counts should be
chosen by trusted configuration, with the total membership size in mind.

## Reproducible measurements

Run:

```sh
cargo test hash_ring::tests::distribution_and_minimal_movement -- --nocapture
cargo test --test hash_ring_process
```

The fixed workload hashes `key-0` through `key-99999` with 256 virtual positions
per node. Measured ownership for four nodes:

| Node | Keys | Share |
|---|---:|---:|
| a | 25,981 | 25.981% |
| b | 24,392 | 24.392% |
| c | 23,483 | 23.483% |
| d | 26,144 | 26.144% |

Adding node `e` changes 22,920 owners (22.920%), all to `e`. The ideal expected
share is 1/(N+1), or 20% when adding a fifth node to four existing nodes.
Removing `e` restores every original owner. Subsequently removing `b` changes
exactly its 24,392 keys; every surviving node retains its original keys.

Finite virtual positions give approximate balance, not exact equal shares.
Tests permit 18.75–31.25% per original node and 15–25% movement on addition.
These are fixed-fixture regression bounds, not guarantees for arbitrary node
names, key sets, or virtual-node counts. Key count does not measure byte size,
request load, or resistance to adversarial keys.

Additional tests pin independently calculated hash values, force collisions,
exercise boundaries and membership edge cases, and compare 1,002 ownership
results across three fresh processes using forward and reversed insertion order.

## TODO: integration and edge-case follow-up

The standalone ring is covered by the tests above. Keep these checks open as
configuration, routing, and data migration are introduced; they are follow-up
integration work, not guarantees provided by the current ownership API.

- [ ] Bound virtual-node counts and total ring memory in configuration. Reject
  oversized settings before allocation, accounting for membership size and
  node-ID length; test clear startup errors for invalid settings.
- [ ] Propagate membership updates explicitly to all ring copies. Test stale
  membership and concurrent lookups during an update; `HashRing::clone()` makes
  an independent snapshot, unlike the shared handles returned by `Engine::clone()`.
- [ ] Validate placement compatibility between nodes: identical stable IDs,
  membership, virtual-node count, encoding, and hash version. Test disagreement
  and coordinated upgrades so incompatible maps cannot silently serve requests.
- [ ] Define router behavior for an empty ring or the removal of the last node.
  Test a clear unavailable response when `owner()` returns `None`.
- [ ] Test actual data migration during node joins and departures, including
  failures and retries, before serving from a new owner. Updating this ring
  changes the ownership calculation only; it does not copy or delete data.
- [ ] Measure byte-size and request-load balance with skewed workloads, multiple
  memberships, and virtual-node counts. The published fixed-key distribution
  test establishes neither load balance nor protection against adversarial keys.
- [ ] Confirm Linux and the minimum supported Rust 1.89 build in PR CI. Local
  validation for this change used Rust 1.98 on macOS.

Already covered by automated tests: zero virtual-node rejection; empty rings;
empty and Unicode IDs/keys; duplicate additions and absent removals; wraparound
and exact position boundaries; forced collisions; independent clones; insertion
order; several virtual-node counts; stable hashes and ownership across processes;
and restricted ownership changes when nodes join or leave. Extend this list and
the regression suite whenever a new edge case is discovered.
