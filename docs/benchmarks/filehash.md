# filehash benchmark report

> These results come from a single host. They are not guarantees.

## Environment

| Item | Value |
|---|---|
| CPU | Intel Xeon E5-2620 v2 @ 2.10 GHz (Ivy Bridge, no SHA-NI), 16 logical CPUs |
| Memory | 62 GiB |
| OS / filesystem | Linux 6.8.0-147-generic x86_64, ext4 on local disk |
| Rust | rustc 1.100.0 (0ef1bc386 2026-11-09) |
| Build profile | `release`: `lto = "thin"`, `codegen-units = 1`, `opt-level = 3`, `strip = true` |
| References | GNU coreutils `sha256sum` 9.4, `b3sum` 1.8.7 |
| Date | 2026-11-25 |
| Cache state | **Warm**: all data in the page cache, so these measure CPU-bound throughput, not disk. |

Workloads: one 1 GiB file of random data, and 10,000 files of 100 KiB random data each (about 1 GiB total). Each command was run 5 times with `hyperfine -N --warmup 1`; values are mean ± σ.

## Results against the engineering targets

| Target (from project plan) | Measured | Status |
|---|---|---|
| ≥ 80% of `sha256sum` throughput (same algorithm, large file) | 5.40 s vs 5.45 s, about 100% | Met |
| ≥ 80% of `b3sum` throughput (same algorithm, large file) | 201 ms vs 203 ms, about 100% | Met |
| < 64 MiB RSS with 4 workers | 7.3 MiB (10,000 files) | Met |

## Single 1 GiB file

| Command | Time |
|---|---|
| `sha256sum big.bin` | 5.445 s ± 0.388 |
| `filehash hash -a sha256 big.bin` | 5.395 s ± 0.351 |
| `b3sum --no-mmap --num-threads 1 big.bin` | 1.482 s ± 0.220 |
| `b3sum big.bin` (default: mmap + all threads) | 202.6 ms ± 14.5 |
| `filehash hash -a blake3 big.bin` (default) | 201.0 ms ± 16.5 |

## 10,000 files × 100 KiB

| Command | Time | vs reference |
|---|---|---|
| `sha256sum <10,000 files>` | 5.502 s ± 0.448 | 1.0× |
| `filehash hash -a sha256 -j 1` | 5.453 s ± 0.414 | 1.0× |
| `filehash hash -a sha256 -j 4` | 1.298 s ± 0.060 | 4.2× |
| `filehash hash -a sha256 -j 16` | 426.7 ms ± 2.9 | 12.9× |
| `b3sum --num-threads 1 <10,000 files>` | 1.949 s ± 0.196 | 1.0× |
| `filehash hash -a blake3 -j 4` | 500.7 ms ± 109.9 | 3.9× |
| `filehash hash -a blake3 -j 16` | 214.2 ms ± 19.4 | 9.1× |

`b3sum` hashes multiple files one after another (its threads work within a file), so `--num-threads 1` is shown as the per-file baseline.

## Memory (peak RSS)

| Workload | Peak RSS |
|---|---|
| sha256, `-j 4`, 10,000 files | 7.3 MiB |
| blake3, `-j 4`, 10,000 files | 7.1 MiB |
| sha256, `-j 16`, 10,000 files | 7.9 MiB |
| sha256, 1 GiB file | 3.6 MiB |
| blake3, 256 MiB file, `--no-mmap` | 3.6 MiB |
| blake3, 1 GiB file, default (mmap) | 1,026 MiB, mostly mapped file pages (`b3sum`: 1,026 MiB) |

## Optimizations recorded during development

The first version used the pure-Rust `sha2` crate and streamed BLAKE3 on a single thread:

| Workload | First version | Final | Change |
|---|---|---|---|
| SHA-256, 1 GiB | 8.07 s (66% of `sha256sum`) | 5.40 s | Switched to *ring* (BoringSSL assembly; AVX/SSSE3 on this CPU, which has no SHA-NI). |
| BLAKE3, 1 GiB | 1.55 s (7.6× slower than `b3sum`) | 201 ms | Memory-mapped multithreaded BLAKE3 for large files when workers would otherwise be idle. |
| SHA-256, 10k files, `-j 16` | 740 ms | 427 ms | Same *ring* change. |

## Microbenchmarks (Criterion, in-memory 1 MiB)

| Benchmark | Throughput |
|---|---|
| sha256 | ~201 MiB/s |
| sha512 | ~213 MiB/s |
| blake3 (single thread) | ~895 MiB/s |
| `manifest::parse_line` | 430 ns/line |

## Startup and binary size

| Metric | Value |
|---|---|
| `filehash --version` | 2.1 ms ± 0.5 |
| Binary (x86_64 Linux, stripped) | 1,068,816 bytes (1.0 MiB) |

## Reproducing

```sh
scripts/bench-filehash.sh [workdir]      # needs ~2.2 GiB free, hyperfine, b3sum
cargo bench -p filehash --bench core
```

## Known limitations

- Warm cache only. Cold-cache runs on spinning or networked storage are I/O-bound, and there extra workers mainly help with latency hiding.
- One CPU model, without SHA-NI. CPUs with SHA extensions will show much higher SHA-256 throughput for both tools.
- Windows and macOS have not been benchmarked.
