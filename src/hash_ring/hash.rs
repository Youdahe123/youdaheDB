//! Stable byte encoding and hashing for placement v1.

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const KEY_DOMAIN: u8 = 0;
const NODE_DOMAIN: u8 = 1;

// Placement v1: FNV-1a 64-bit followed by the MurmurHash3 64-bit finalizer.
// All arithmetic wraps modulo 2^64. The finalizer spreads similar input bytes
// across the ring. This is a fixed non-cryptographic hash, not RandomState or
// Rust's unspecified DefaultHasher. Constants/encoding must not change silently.
fn stable_hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51afd7ed558ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
    hash ^ (hash >> 33)
}

pub(super) fn key_hash(key: &str) -> u64 {
    stable_hash([KEY_DOMAIN].into_iter().chain(key.bytes()))
}

pub(super) fn node_hash(node_id: &str, virtual_index: u32) -> u64 {
    // Domain byte 1 distinguishes node positions from keys (domain byte 0).
    // The fixed-width trailing index makes this encoding unambiguous.
    stable_hash(
        [NODE_DOMAIN]
            .into_iter()
            .chain(node_id.bytes())
            .chain(virtual_index.to_le_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::{key_hash, node_hash};

    #[test]
    fn placement_v1_known_hashes() {
        // Independently calculated vectors pin encoding and arithmetic, rather
        // than merely checking that the implementation agrees with itself.
        assert_eq!(key_hash(""), 0xb9034ad37056f5fb);
        assert_eq!(key_hash("user:123"), 0xfec29070cebb06fe);
        assert_eq!(key_hash("東京"), 0xe711af626c912940);
        assert_eq!(node_hash("node-a", 0), 0xf505711d9f2cfb7e);
        assert_eq!(node_hash("node-a", 255), 0xced3be93214d0a92);
        assert_eq!(node_hash("", 0), 0xebdb2a4c2957fac0);
    }
}
