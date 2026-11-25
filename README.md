<div align="center">

# rust-sysutils

**Small, fast, cross-platform system and filesystem utilities written in Rust.**

[![CI](https://github.com/mr-aybee/rust-sysutils/actions/workflows/ci.yml/badge.svg)](https://github.com/mr-aybee/rust-sysutils/actions/workflows/ci.yml)
![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

</div>

rust-sysutils is a collection of single-purpose command-line tools for developers, system administrators and DevOps engineers. Each tool is one self-contained binary with bounded memory use, human and JSON output, and exit codes you can rely on in scripts and CI.

## Utilities

| Utility | Status | Purpose |
|---|---|---|
| [**filehash**](docs/utilities/filehash.md) | ✅ 0.1.0 | Parallel hashing (SHA-256/512, BLAKE3) and manifest verification, compatible with `sha256sum`/`b3sum` |
| [**disklens**](docs/utilities/disklens.md) | ✅ 0.1.0 | Parallel disk usage analysis: largest directories and files, `du`-identical totals |

More utilities (filesystem watching, process inspection and others) will be added here only once they are implemented and tested.

## Key features

- **Self-contained binaries**: no runtime to install; each is about 1 MiB.
- **Interoperable**: filehash manifests work with `sha256sum -c`, and filehash verifies `sha256sum` and `b3sum` output.
- **Bounded resources**: fixed-size buffers, configurable worker counts, no unbounded queues.
- **Safe by default**: read-only operation, symlinks not followed during walks, atomic output files.
- **Automation-friendly**: `--json`, stable exit codes, diagnostics on stderr.

## Supported platforms

| OS | x86_64 | ARM64 |
|---|---|---|
| Linux | ✅ tested in CI | ✅ tested in CI |
| Windows | ✅ tested in CI | ✅ tested in CI |
| macOS | 🔨 built in CI (cross-compiled, not tested) | ✅ tested in CI |

## Installation

Prebuilt binaries with SHA-256 checksums are attached to each [release](https://github.com/mr-aybee/rust-sysutils/releases).

From source (Rust 1.88+):

```sh
cargo install --git https://github.com/mr-aybee/rust-sysutils filehash
```

## Quick start

```sh
filehash hash dist/ -o dist/SHA256SUMS           # create a manifest (sha256sum-compatible)
filehash verify dist/SHA256SUMS                  # verify it
filehash hash -a blake3 /data -o data.b3         # BLAKE3, parallel
filehash verify data.b3 --report-extra -q        # show only problems, incl. untracked files
disklens / -x -d 2 -t 20                         # what is filling the root filesystem?
disklens ~ -e node_modules -e .cache --json      # machine-readable usage report
```

## Use cases

- **Release engineering**: publish checksums that anyone can verify with plain coreutils.
- **Backup and transfer integrity**: verify large datasets after copying, and find files added since the last run.
- **CI drift checks**: fail a build when generated or vendored files change unexpectedly.
- **Storage triage**: find what is filling a disk in seconds, with numbers that match `du`.

## Performance

Measured on an Intel Xeon E5-2620 v2 running Linux. Full methodology: [filehash](docs/benchmarks/filehash.md), [disklens](docs/benchmarks/disklens.md).

| Workload | Reference | Result |
|---|---|---|
| filehash: SHA-256, one 1 GiB file (warm) | `sha256sum` 5.45 s | 5.40 s |
| filehash: BLAKE3, one 1 GiB file (warm) | `b3sum` 203 ms | 201 ms |
| filehash: SHA-256, 10,000 × 100 KiB files | `sha256sum` 5.50 s | 0.43 s (`-j 16`) |
| disklens: 100,000 files, warm | `du -s` 505 ms | 57 ms |
| disklens: 100,000 files, cold cache | `du -s` 2.99 s | 0.40 s |
| disklens: `/usr`, 131k files | `du -sx` 1.02 s | 0.14 s, 12.8 MiB RSS |

## Architecture

See [docs/architecture/overview.md](docs/architecture/overview.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
