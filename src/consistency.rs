// TODO: drop this allow once the stubs below are implemented
#![allow(unused_variables, dead_code)]

// Per-request consistency: where a read is allowed to be served from.
// The whole tradeoff in one enum — leader reads are correct but serialized,
// follower reads scale out but can return stale data.

#[derive(Clone, Copy, PartialEq)]
pub enum ConsistencyLevel {
    // read from the leader: always sees the latest committed write
    Strong,
    // read from any replica: cheaper and parallel, may lag behind
    Eventual,
}

impl ConsistencyLevel {
    // parses the CLI flag / client argument ("strong" | "eventual")
    pub fn from_str(s: &str) -> Option<ConsistencyLevel> {
        match s.to_ascii_lowercase().as_str() {
            "strong" => Some(ConsistencyLevel::Strong),
            "eventual" => Some(ConsistencyLevel::Eventual),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ConsistencyLevel::Strong => "strong",
            ConsistencyLevel::Eventual => "eventual",
        }
    }
}

// which replica a read got routed to
pub enum ReadTarget {
    Leader,
    Follower(usize),
}

// picks the replica for each read according to the configured level
pub struct ReadRouter {
    level: ConsistencyLevel,
    num_replicas: usize,
    // round-robins follower reads so no single replica takes the whole load
    next_follower: usize,
}

impl ReadRouter {
    pub fn new(level: ConsistencyLevel, num_replicas: usize) -> ReadRouter {
        ReadRouter { level, num_replicas, next_follower: 0 }
    }

    // DESIGN: this one method is the entire consistency tradeoff. Strong must
    // go to the leader, since only the leader is guaranteed to have every
    // committed write. Eventual spreads reads over the followers, which is
    // where the throughput win comes from — and also where stale reads come
    // from. A single-node cluster has no followers to fall back on.
    pub fn route_read(&mut self) -> ReadTarget {
        todo!("Strong -> ReadTarget::Leader; Eventual -> round-robin a \
               follower via self.next_follower, falling back to Leader when \
               there are no followers")
    }

    // writes always go to the leader regardless of level — this exists so the
    // call site reads symmetrically with route_read
    pub fn route_write(&self) -> ReadTarget {
        ReadTarget::Leader
    }

    // DESIGN: how lag gets modelled decides what the demo can actually show.
    // A fixed delay is easy but never surprises anyone; making follower N lag
    // by N writes lets you print a leader read and a follower read side by
    // side and have them genuinely disagree, which is the point.
    pub fn replica_lag(&self, replica: usize) -> u64 {
        todo!("return how many committed entries this follower is behind")
    }
}
