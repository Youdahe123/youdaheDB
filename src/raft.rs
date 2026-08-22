// TODO: drop this allow once the stubs below are implemented
#![allow(unused_variables, dead_code)]

// Raft-style log replication, simplified: fixed leader, no elections.
// Writes are only acknowledged once a majority of the cluster has the entry.

// what a single log entry asks the state machine to do
pub enum Command {
    Put { key: String, value: String },
    Delete { key: String },
}

// one slot in the replicated log. term + index together identify an entry
// uniquely across the cluster — that pair is what consistency checks compare.
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub command: Command,
}

// a node is either the single fixed leader or a follower replicating from it
#[derive(PartialEq)]
pub enum NodeRole {
    Leader,
    Follower,
}

// what the leader sends to each follower to push new entries
pub struct AppendEntriesRequest {
    pub term: u64,
    pub leader_id: String,
    // index/term of the entry immediately BEFORE the new ones — the follower
    // rejects the request if its log doesn't match here (the consistency check)
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<LogEntry>,
    // how far the leader has committed, so the follower can apply too
    pub leader_commit: u64,
}

pub struct AppendEntriesResponse {
    pub term: u64,
    pub success: bool,
    // highest index the follower now has — the leader uses this to track quorum
    pub match_index: u64,
}

pub struct RaftNode {
    pub id: String,
    pub role: NodeRole,
    pub current_term: u64,
    pub log: Vec<LogEntry>,
    // highest index known to be replicated on a majority
    pub commit_index: u64,
    // highest index actually applied to the storage engine
    pub last_applied: u64,
    // addresses of the other nodes in this cluster
    pub peers: Vec<String>,
    // leader only: highest index confirmed on each peer
    pub match_index: std::collections::HashMap<String, u64>,
}

impl RaftNode {
    // creates a node with an empty log at term 0
    pub fn new(id: &str, peers: Vec<String>, role: NodeRole) -> RaftNode {
        let match_index = peers.iter().map(|peer| (peer.clone(), 0)).collect();
        RaftNode {
            id: id.to_string(),
            role,
            current_term: 0,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            peers,
            match_index,
        }
    }

    pub fn is_leader(&self) -> bool {
        self.role == NodeRole::Leader
    }

    // leader only: appends the command locally and returns its log index.
    // NOT committed yet — that needs replicate() to reach a majority.
    pub fn propose(&mut self, command: Command) -> Result<u64, String> {
        if !self.is_leader() {
            return Err(format!("node {} is not the leader", self.id));
        }

        let (last_index, _) = self.last_log_index_and_term();
        let index = last_index + 1;
        self.log.push(LogEntry { term: self.current_term, index, command });
        Ok(index)
    }

    // DESIGN: this is where the network would go. Decide what a peer even is
    // here — an in-process RaftNode (simplest, and enough to demonstrate
    // quorum), a thread, or a real TCP connection. Whatever you pick, a peer
    // that fails to ack must NOT count toward the quorum, and the leader has
    // to keep working when a minority is down.
    pub fn replicate(&mut self, index: u64) -> usize {
        todo!("send AppendEntries to each peer, record successes in \
               self.match_index, and return the ack count including self")
    }

    // DESIGN: commit_index only moves forward, and only to an index a MAJORITY
    // holds — that is the entire safety argument for why a committed entry
    // survives losing a minority of nodes. Real Raft adds one more rule: a
    // leader may only commit entries from its OWN term this way, otherwise a
    // committed entry can still be overwritten. Worth handling even here.
    pub fn advance_commit_index(&mut self) {
        todo!("find the highest index present on quorum_size() nodes and move \
               self.commit_index up to it (never backwards)")
    }

    // DESIGN: the log consistency check is the heart of Raft. Reject if the
    // leader's term is stale, or if this node has no entry matching
    // prev_log_index/prev_log_term — the leader then retries at a lower index
    // until the logs line up. On a conflicting entry, truncate everything from
    // that point forward before appending: a follower's divergent tail is
    // always discarded, never merged.
    pub fn handle_append_entries(&mut self, req: AppendEntriesRequest) -> AppendEntriesResponse {
        todo!("term check -> consistency check at prev_log_index -> truncate \
               conflicts -> append entries -> advance commit_index to \
               min(leader_commit, last local index)")
    }

    // applies every committed-but-unapplied entry to the storage engine.
    // Entries are applied in index order and exactly once, which is what makes
    // every replica's state machine identical.
    pub fn apply_committed(&mut self, store: &mut crate::lsm::LsmTree) -> Result<(), std::io::Error> {
        while self.last_applied < self.commit_index {
            let next = self.last_applied + 1;

            // log is 0-indexed, entry indices start at 1
            if let Some(entry) = self.log.get((next - 1) as usize) {
                match &entry.command {
                    Command::Put { key, value } => store.put(key, value)?,
                    Command::Delete { key } => store.delete(key)?,
                }
            }

            self.last_applied = next;
        }

        Ok(())
    }

    // majority of the whole cluster (peers + self), so 3 nodes -> 2, 5 -> 3
    fn quorum_size(&self) -> usize {
        (self.peers.len() + 1) / 2 + 1
    }

    // (0, 0) for an empty log, which makes the first consistency check pass
    fn last_log_index_and_term(&self) -> (u64, u64) {
        self.log.last().map_or((0, 0), |entry| (entry.index, entry.term))
    }
}
