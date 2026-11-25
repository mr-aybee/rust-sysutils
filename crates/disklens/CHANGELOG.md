# Changelog — disklens

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-25

### Added
- Parallel recursive scan (one task per directory on a work-stealing pool) with sizes identical to GNU `du` (on-disk: `du -B1`; apparent: `du -b`).
- Tree view of the largest directories to `--depth`, limited to `--limit` children per directory, plus the `--top` largest files.
- Ranking by on-disk (allocated) size, or apparent size with `--apparent`.
- Hard links counted once; symbolic links counted but never followed; `-x` stays on one filesystem; `--exclude` globs (`*`, `?`, `**`) on names or relative paths.
- Unreadable entries are counted and reported, and the exit status is 1, as with `du`.
- `--json` output (tree pruned to `--depth`), `--bytes`, `-j` worker threads.
