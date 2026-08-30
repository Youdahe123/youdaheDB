// CRDTs. The alternative to raft: replicas take writes with no coordination
// and still converge, because merge() doesn't care about order or duplicates.
// Counters (G/PN) and a last-writer-wins register.
// Empty for now.
