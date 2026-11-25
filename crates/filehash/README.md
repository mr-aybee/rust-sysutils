# filehash

Parallel file hashing and integrity verification. It writes and verifies manifests in the same format as `sha256sum` and `b3sum`, so it works with existing tooling. It is much faster on many files, and memory stays bounded.

```console
$ filehash hash -a blake3 dist/ -o B3SUMS
filehash: 128 file(s) hashed, 0 error(s), 0 symlink(s) skipped

$ filehash verify B3SUMS --report-extra -q
FAILED   dist/app.tar.gz: expected 52b7…, got 8e4c…
EXTRA    dist/debug.log
--- 128 checked: 127 ok, 1 failed, 0 missing, 0 error(s), 1 extra
```

## Install

Download a binary from [Releases](https://github.com/mr-aybee/rust-sysutils/releases) (see `SHA256SUMS`), or:

```sh
cargo install --git https://github.com/mr-aybee/rust-sysutils filehash
```

## Commands

| Command | Purpose |
|---|---|
| `filehash hash PATH... [-a sha256\|sha512\|blake3] [-o FILE] [--tag]` | Hash files and directories into a manifest |
| `filehash verify MANIFEST [--report-extra] [--root DIR]` | Check files against a manifest |

Exit codes: `0` ok, `1` mismatch, missing, unreadable or extra files, `2` usage error.

Full documentation: [docs/utilities/filehash.md](../../docs/utilities/filehash.md). Benchmarks: [docs/benchmarks/filehash.md](../../docs/benchmarks/filehash.md).

## License

MIT OR Apache-2.0.
