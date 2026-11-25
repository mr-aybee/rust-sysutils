# disklens

Find what's using your disk, fast. disklens scans directory trees in parallel, gives the same totals as `du`, and shows the largest directories *and* the largest files in one view, with JSON output for automation.

```console
$ disklens ~/.cargo -d 2 -l 3 -t 3
    ON DISK       %       FILES  PATH
    2.1 GiB  100.0%      87,998  /home/user/.cargo
    1.9 GiB   91.4%      84,677  ├─ registry/
    1.6 GiB   77.3%      81,855  │  ├─ src/
  226.0 MiB   10.5%       1,696  │  ├─ cache/
   75.6 MiB    3.5%       1,125  │  └─ index/
  110.6 MiB    5.2%           9  ├─ bin/
   ...

Largest files:
   47.8 MiB  advisory-db/.git/objects/pack/pack-9f2b….pack
   30.9 MiB  bin/cargo-nextest
   23.0 MiB  bin/cargo-audit

Total: 1.8 GiB apparent, 2.1 GiB on disk, 87,998 files, 18,368 directories
```

## Install

Download a binary from [Releases](https://github.com/mr-aybee/rust-sysutils/releases) (see `SHA256SUMS`), or:

```sh
cargo install --git https://github.com/mr-aybee/rust-sysutils disklens
```

## Usage

```sh
disklens [PATH] [-d DEPTH] [-l LIMIT] [-t TOP] [-a] [-e GLOB]... [-x] [--json]
```

Exit codes: `0` ok, `1` some entries could not be read (totals are partial) or the path doesn't exist, `2` usage error.

Full documentation: [docs/utilities/disklens.md](../../docs/utilities/disklens.md). Benchmarks: [docs/benchmarks/disklens.md](../../docs/benchmarks/disklens.md).

## License

MIT OR Apache-2.0.
