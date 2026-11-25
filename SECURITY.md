# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Report them privately via
GitHub's [private vulnerability reporting](https://github.com/mr-aybee/rust-sysutils/security/advisories/new).

Include the affected utility and version, platform, reproduction steps and impact.
You should receive an acknowledgement within 7 days.

## Supported versions

Only the latest release of each utility receives security fixes.

## Scope and intended use

These utilities operate on local files and system information. Security-relevant defaults:

- Read-only by default; anything that modifies files requires an explicit flag.
- Symbolic links are not followed during directory traversal unless requested.
- No shell commands are executed.
- Memory use is bounded (fixed-size read buffers, bounded worker counts).
- Dependencies are audited in CI with `cargo-deny` (RustSec advisories and licenses).

Note: a hash manifest proves integrity only if the manifest itself is trustworthy
(for example, signed or obtained over an authenticated channel).
