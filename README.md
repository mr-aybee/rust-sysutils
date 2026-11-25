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

More utilities (disk usage analysis, filesystem watching, process inspection and others) will be added here only once they are implemented and tested.

## Key features

- **Self-contained binaries**: no runtime to install; filehash is about 1 MiB.
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
```

## Use cases

- **Release engineering**: publish checksums that anyone can verify with plain coreutils.
- **Backup and transfer integrity**: verify large datasets after copying, and find files added since the last run.
- **CI drift checks**: fail a build when generated or vendored files change unexpectedly.

## Performance

Measured on an Intel Xeon E5-2620 v2 running Linux, warm cache. Full methodology: [docs/benchmarks/filehash.md](docs/benchmarks/filehash.md).

| Workload | Reference | filehash |
|---|---|---|
| SHA-256, one 1 GiB file | `sha256sum` 5.45 s | 5.40 s |
| BLAKE3, one 1 GiB file | `b3sum` 203 ms | 201 ms |
| SHA-256, 10,000 × 100 KiB files | `sha256sum` 5.50 s | 0.43 s (`-j 16`) |
| Peak RSS, 10,000 files, 4 workers | — | 7.3 MiB |

## Architecture

See [docs/architecture/overview.md](docs/architecture/overview.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
