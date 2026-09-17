use std::process::Command;
use youdaheDB::HashRing;

// Invoke this same test executable in fresh processes. Comparing a marked
// payload avoids depending on the test runner's timing or output formatting.
#[test]
fn process_probe() {
    let Ok(order) = std::env::var("YOUDAHEDB_RING_PROBE_ORDER") else {
        return;
    };
    let mut ring = HashRing::default();
    let nodes = if order == "reverse" {
        ["東京", "node-c", "node-b", "node-a"]
    } else {
        ["node-a", "node-b", "node-c", "東京"]
    };
    for node in nodes {
        ring.add_node(node);
    }
    for key in ["".to_owned(), "🔑".to_owned()]
        .into_iter()
        .chain((0..1000).map(|i| format!("user:{i}")))
    {
        println!("RING_OWNER={}", ring.owner(&key).unwrap());
    }
}

#[test]
fn ownership_is_stable_across_processes() {
    let executable = std::env::current_exe().unwrap();
    let probe = |order| {
        let output = Command::new(&executable)
            .args(["--exact", "process_probe", "--nocapture"])
            .env("YOUDAHEDB_RING_PROBE_ORDER", order)
            .output()
            .expect("start independent ring process");
        assert!(output.status.success(), "child failed: {output:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("RING_OWNER="))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let first = probe("forward");
    assert_eq!(first.len(), 1002);
    assert_eq!(first, probe("forward"));
    assert_eq!(first, probe("reverse"));
}
