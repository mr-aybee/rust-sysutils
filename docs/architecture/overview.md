# Architecture overview

```text
rust-sysutils/
├── crates/
│   └── filehash/     library + binary
├── docs/             architecture, utility docs, benchmark reports
└── scripts/          reproducible benchmark harnesses
```

Each utility is an independent crate with its own version, changelog, tests and release tag (`<crate>-vX.Y.Z`). A private `sys-common` crate will be added only once a second utility needs the same code.

## filehash

| Decision | Reason |
|---|---|
| Plain scoped threads, no async runtime | Hashing is CPU- and disk-bound. An atomic work index plus `std::thread::scope` is enough and adds no dependencies. |
| One reusable buffer per worker; result slots by index | Memory is bounded by worker count, and output order is deterministic. |
| *ring* for SHA-2, `blake3` crate for BLAKE3 | *ring* has assembly-optimized SHA-2 for x86 and ARM. The pure-Rust `sha2` ran at 66% of `sha256sum` on a CPU without SHA-NI. |
| BLAKE3 memory-maps large files only when workers would otherwise be idle | Matches `b3sum` for single large files without oversubscribing CPUs on many-file runs. `--no-mmap` gives strictly bounded memory. |
| GNU/BSD manifest compatibility, refusing to guess ambiguous algorithms | Works with existing release tooling. A wrong guess would report false mismatches. |
| Atomic `-o` writes (temporary file, fsync, rename) | An interrupted run never leaves a truncated manifest. |
