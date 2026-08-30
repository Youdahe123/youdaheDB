// Consistency level. Picks who answers a read: STRONG goes to the leader and
// is always current, EVENTUAL goes to a follower and is faster but can be
// stale. Writes always go to the leader.
// Empty for now.
