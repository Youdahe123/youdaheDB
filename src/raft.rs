// Replication. One leader, a replicated write log, and a write only counts as
// committed once a majority of nodes has it — that's what survives losing a
// minority. Committed entries get applied into the lsm.rs storage engine.
// Empty for now.
