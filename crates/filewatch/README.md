# filewatch

Watch files and directories. Print debounced changes, or run a command when something changes: safely (no shell), never overlapping, with the changed paths on stdin.

```console
$ filewatch src -i '*.rs'
2026-11-25T15:05:50Z  CREATED  lib/new.rs
2026-11-25T15:05:52Z  MODIFIED main.rs

$ filewatch src -i '*.rs' -- cargo test        # rerun tests on change
$ filewatch . -e target -r -- cargo run        # restart a server on change
$ filewatch inbox --once -- ./ingest.sh        # wait for the first change, process it, exit
```

## Install

Download a binary from [Releases](https://github.com/mr-aybee/rust-sysutils/releases), or:

```sh
cargo install --git https://github.com/mr-aybee/rust-sysutils filewatch
```

Exit codes: `0` stopped normally (Ctrl+C, or after `--once`), `1` a path could not be watched, `2` usage error.

Full documentation: [docs/utilities/filewatch.md](../../docs/utilities/filewatch.md). Benchmarks: [docs/benchmarks/filewatch.md](../../docs/benchmarks/filewatch.md).

## License

MIT OR Apache-2.0.
