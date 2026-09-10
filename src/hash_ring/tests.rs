use super::{HashRing, Position, RingError};

#[test]
fn membership_and_configuration() {
    assert_eq!(HashRing::new(0).unwrap_err(), RingError::ZeroVirtualNodes);
    let mut ring = HashRing::new(3).unwrap();
    assert_eq!(ring.owner("key"), None);
    assert!(!ring.remove_node("missing"));
    for node in ["", "node-a", "東京"] {
        assert!(ring.add_node(node));
    }
    assert_eq!(ring.positions.len(), 9);
    let positions = ring.positions.clone();
    assert!(!ring.add_node("node-a"));
    assert_eq!(ring.positions, positions);
    assert!(ring.remove_node("node-a"));
    assert!(!ring.remove_node("node-a"));
    assert!(ring.remove_node(""));
    for key in ["", "key", "🔑", "a\0b"] {
        assert_eq!(ring.owner(key), Some("東京"));
    }
    assert!(ring.remove_node("東京"));
    assert_eq!(ring.owner("key"), None);
    assert_eq!(HashRing::default().virtual_nodes, 256);
}

#[test]
fn clockwise_boundaries_wraparound_and_collisions() {
    let mut ring = HashRing::new(1).unwrap();
    // Force a collision: the first node in stable ID order must win even
    // when positions arrive in the reverse order. No entry may be dropped.
    ring.positions = vec![
        Position {
            hash: 40,
            node_id: "b".into(),
            virtual_index: 0,
        },
        Position {
            hash: 40,
            node_id: "a".into(),
            virtual_index: 1,
        },
        Position {
            hash: 10,
            node_id: "c".into(),
            virtual_index: 0,
        },
    ];
    ring.positions.sort_unstable();
    assert_eq!(ring.positions.len(), 3);
    for (hash, owner) in [
        (0, "c"),
        (10, "c"),
        (11, "a"),
        (40, "a"),
        (41, "c"),
        (u64::MAX, "c"),
    ] {
        assert_eq!(ring.owner_at(hash), Some(owner));
    }
    ring.nodes.insert("a".into());
    ring.remove_node("a");
    assert_eq!(ring.owner_at(40), Some("b"));
}

#[test]
fn insertion_order_and_readdition_do_not_change_ownership() {
    let mut first = HashRing::default();
    let mut second = HashRing::default();
    for node in ["a", "b", "c"] {
        first.add_node(node);
    }
    for node in ["c", "b", "a"] {
        second.add_node(node);
    }
    assert_eq!(first.positions, second.positions);
    second.remove_node("b");
    second.add_node("b");
    assert_eq!(first.positions, second.positions);
    for i in 0..1000 {
        assert_eq!(
            first.owner(&format!("key-{i}")),
            second.owner(&format!("key-{i}"))
        );
    }
}

#[test]
fn distribution_and_minimal_movement() {
    const KEYS: usize = 100_000;
    let mut ring = HashRing::default();
    for node in ["a", "b", "c", "d"] {
        ring.add_node(node);
    }
    let keys: Vec<_> = (0..KEYS).map(|i| format!("key-{i}")).collect();
    let before: Vec<_> = keys
        .iter()
        .map(|key| ring.owner(key).unwrap().to_owned())
        .collect();
    let mut counts = [0; 4];
    for owner in &before {
        counts[(owner.as_bytes()[0] - b'a') as usize] += 1;
    }
    // Equal ownership is approximate with finitely many virtual positions.
    // Allow +/-25% relative to the ideal 25,000 keys per node.
    for count in counts {
        assert!((18_750..=31_250).contains(&count), "counts: {counts:?}");
    }
    ring.add_node("e");
    let mut moved = 0;
    for (key, old) in keys.iter().zip(&before) {
        let new = ring.owner(key).unwrap();
        if new != old {
            assert_eq!(new, "e");
            moved += 1;
        }
    }
    // Adding the fifth node should move about 20%, +/-5 percentage points.
    assert!((15_000..=25_000).contains(&moved), "moved {moved}");
    ring.remove_node("e");
    for (key, old) in keys.iter().zip(&before) {
        assert_eq!(ring.owner(key), Some(old.as_str()));
    }
    ring.remove_node("b");
    let mut removed_moved = 0;
    for (key, old) in keys.iter().zip(&before) {
        let new = ring.owner(key).unwrap();
        assert_ne!(new, "b");
        if new != old {
            assert_eq!(old, "b");
            removed_moved += 1;
        }
    }
    assert_eq!(removed_moved, counts[1]);
    println!("100000 keys, 256 vnodes/node: counts={counts:?}; add fifth moved={moved}; remove b moved={removed_moved}");
}

#[test]
fn configured_rings_match_a_linear_lookup_reference() {
    for virtual_nodes in [1, 2, 17, 256] {
        let mut ring = HashRing::new(virtual_nodes).unwrap();
        for node in ["a", "b", "東京"] {
            ring.add_node(node);
        }
        // A linear minimum search supplies a reference independent of binary
        // search and the position vector's sorting implementation.
        for i in 0..1000 {
            let key = format!("key-{i}");
            let hash = super::hash::key_hash(&key);
            let expected = ring
                .positions
                .iter()
                .filter(|position| position.hash >= hash)
                .min()
                .or_else(|| ring.positions.iter().min())
                .map(|position| position.node_id.as_str());
            assert_eq!(
                ring.owner(&key),
                expected,
                "vnodes={virtual_nodes}, key={key}"
            );
        }
        let snapshot = ring.clone();
        ring.remove_node("b");
        assert!(snapshot.nodes.contains("b"));
        assert!(!ring.nodes.contains("b"));
    }
}
