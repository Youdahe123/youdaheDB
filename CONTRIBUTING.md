# Contributing

## The one rule

**Standard library only.** No crates, in the database or in its tests. If you
need a B-tree, a hash, a file format or a protocol parser, write it. That
constraint is the point of the project — a dependency that hides the
interesting decision also hides the thing worth learning.

The website under `web/` is the same: no build step, no framework, no bundler.

## Getting it running

```sh
git clone https://github.com/Youdahe123/youdaheDB
cd youdaheDB
cargo test
```

Nothing to install beyond a Rust toolchain. No config file, no services.

To run the site locally:

```sh
python3 -m http.server 8777 --directory web
```

## Before you open a PR

- `cargo test` passes.
- New behaviour ships with a test. The interesting tests are the ones that
  assert the failure the change exists to prevent, not just the happy path.
- `cargo fmt` has been run.

## What makes a good change here

The engine is built bottom-up and each layer is meant to work end to end before
the next one starts. Issues carry the reasoning for where they sit in that
order — read that before proposing something from further up the stack.

Design disagreements belong in an issue before the code.

## Security

Do not open a public issue for a security problem. See [SECURITY.md](SECURITY.md).
