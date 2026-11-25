# Changelog — filehash

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-25

### Added
- `hash`: SHA-256, SHA-512 and BLAKE3 over files, directories (recursive, sorted) and stdin, using parallel workers with one fixed-size buffer each.
- Manifests in GNU `sha256sum`/`b3sum` format (with escaping for `\` and newlines) or BSD tag format (`--tag`); `-o` writes atomically.
- `verify`: checks GNU or BSD manifests (including the binary `*` marker and CRLF line endings); reports `OK`, `FAILED`, `MISSING`, `ERROR`, and optionally `EXTRA` files not in the manifest (`--report-extra`).
- Algorithm inference from the manifest name or digest length; ambiguous 64-hex-digit manifests require `--algorithm`.
- Symbolic links are not followed by default (`-L` to follow, with cycle detection); hidden files are skipped unless `--hidden` is given.
- BLAKE3 uses memory-mapped multithreaded hashing for large files when workers would otherwise be idle; `--no-mmap` forces streaming.
- `--json` output; exit codes 0 ok, 1 problems found, 2 usage error.
