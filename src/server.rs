// Network layer. TCP server speaking RESP (the Redis protocol) so redis-cli
// and the redis crate work against it unmodified — which is what makes the
// benchmark fair. Turns GET/SET/DEL into calls on the sharded store.
// Empty for now.
