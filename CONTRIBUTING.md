# Contributing

Thanks for your interest in improving rust-sysutils.

## Development setup

- Install stable Rust (MSRV is **1.88**, recorded in `Cargo.toml`).
- Each utility lives in `crates/<name>` and builds, tests and installs on its own:
  `cargo run -p filehash -- hash .`.

## Before opening a pull request

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Tests must not require internet access; use local fixtures (see `crates/filehash/tests/cli.rs`).

## Guidelines

- Keep dependencies minimal and justify new ones in the PR description.
- `unsafe` code is denied workspace-wide. A change that needs it must explain why and document the invariants.
- Do not report a result the tool cannot prove (for example, an unreadable file is an error, never a match).
- Update the utility's `CHANGELOG.md` and docs under `docs/utilities/` when behavior changes.
- Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat(netprobe): ...`, `fix(netprobe): ...`).
- Benchmark claims must come from a reproducible run; include the command and environment.

By contributing you agree that your contributions are dual-licensed under MIT OR Apache-2.0.
