# filewatch benchmark report

> These results come from a single host. They are not guarantees.

## Environment

| Item | Value |
|---|---|
| CPU | Intel Xeon E5-2620 v2 @ 2.10 GHz, 16 logical CPUs |
| OS / filesystem | Linux 6.8.0-147-generic x86_64, ext4 |
| inotify | `max_queued_events` 16384 |
| Rust | rustc 1.100.0 (0ef1bc386 2026-11-09) |
| Build profile | `release` (thin LTO, `codegen-units = 1`, stripped) |
| Date | 2026-11-25 |

## Method

`examples/latency.rs` starts `filewatch DIR --json -d 0ms`, creates N files at a fixed rate, and timestamps each `created` line as it is read from filewatch's stdout. Latency runs from the file write returning to the line being received, so it includes the kernel event, the notify thread, coalescing, filtering and output.

## Results against the engineering targets

| Target (from project plan) | Measured | Status |
|---|---|---|
| p95 event delivery < 100 ms, excluding the configured debounce | p95 0.18–0.38 ms | Met |
| 1,000 events/s with no silent loss | 5,000 events at 1,000/s and 20,000 at 5,000/s: all received | Met |

## Latency and loss (debounce 0)

| Files | Rate | Received | Missing | p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|---|
| 1,000 | 100/s | 1,000 | 0 | 0.24 ms | 0.38 ms | 0.90 ms | 7.49 ms |
| 5,000 | 1,000/s | 5,000 | 0 | 0.23 ms | 0.33 ms | 0.38 ms | 0.76 ms |
| 20,000 | 5,000/s | 20,000 | 0 | 0.11 ms | 0.18 ms | 0.22 ms | 1.00 ms |

## Overflow behaviour

With `max_queued_events` temporarily set to 64, a steady stream of 3,000 creates produced no overflow: the watcher thread kept up, and all 3,000 events were reported. With filewatch paused (`SIGSTOP`) while 500 files were created, the kernel queue overflowed. After resuming, filewatch reported 22 `created` events plus one `overflow` event for the root. Nothing was lost silently.

## Idle cost

Watching a tree of 10,101 directories for 30 s with no changes:

| Metric | Value |
|---|---|
| Peak RSS | 10.3 MiB |
| CPU | 0.66 s over 30 s, nearly all of it startup (registering 10,101 inotify watches). The idle loop blocks on the channel and does not poll. |

## Startup and binary size

| Metric | Value |
|---|---|
| Binary (x86_64 Linux, stripped) | 1,106,952 bytes (1.1 MiB) |

## Reproducing

```sh
scripts/bench-filewatch.sh [workdir]
```

## Known limitations

- Linux and ext4 only; macOS (FSEvents) and Windows have not been benchmarked.
- No comparison with watchexec, entr or inotifywait (none were installed on the reference host).
