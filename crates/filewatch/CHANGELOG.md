# Changelog — filewatch

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-25

### Added
- Recursive (or `-N` non-recursive) watching via native OS events: inotify, FSEvents, ReadDirectoryChangesW.
- Debounced batches (`-d`) with per-path coalescing. Create + modify becomes `created`; create + remove disappears; remove + create becomes `modified`; renames report both sides.
- `--include`/`--exclude` globs. A name pattern also excludes everything below a matching directory. Version-control directories are ignored by default.
- Text or JSON Lines output.
- Command execution after `--`, run directly (never through a shell), with changed paths on stdin and `FILEWATCH_CHANGED` in the environment. Runs never overlap: changes during a run are queued into one follow-up run, or the run is killed and restarted with `--restart`.
- `--run-on-start` and `--once`.
- Kernel queue overflow is reported as an `overflow` event for the watched root and triggers the command; it is never dropped silently.
