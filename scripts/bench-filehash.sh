#!/usr/bin/env bash
# End-to-end benchmark harness for filehash (Linux). Requires: hyperfine, /usr/bin/time, sha256sum, b3sum.
# Usage: scripts/bench-filehash.sh [workdir]   (needs ~2.2 GiB free; all runs use a warm page cache)
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q -p filehash
BIN=$PWD/target/release/filehash
WORK=${1:-$(mktemp -d)}; mkdir -p "$WORK"; trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
echo "## environment"
echo "cpu: $(lscpu | sed -n 's/^Model name:[[:space:]]*//p') ($(nproc) logical)"
echo "kernel: $(uname -sr)"; rustc --version; sha256sum --version | head -1; b3sum --version
echo "binary size: $(stat -c %s "$BIN") bytes"
echo "filesystem: $(df -T . | awk 'NR==2{print $2}')"

head -c 1073741824 /dev/urandom > big.bin
mkdir many && for i in $(seq 0 9999); do head -c 102400 /dev/urandom > "many/f$(printf %05d $i)"; done
cat big.bin many/* > /dev/null   # warm the page cache

echo "## startup"
hyperfine -N --warmup 20 --runs 200 "$BIN --version" 2>/dev/null | grep -E "Time"

echo "## single 1 GiB file (warm cache)"
hyperfine -N --warmup 1 --runs 5 \
  "sha256sum big.bin" "$BIN hash -a sha256 big.bin" \
  "b3sum --no-mmap --num-threads 1 big.bin" "$BIN hash -a blake3 big.bin" \
  "b3sum big.bin" 2>/dev/null | grep -E "Benchmark|Time"

echo "## 10,000 files x 100 KiB (warm cache)"
hyperfine -N --warmup 1 --runs 5 \
  "sha256sum -- $(echo many/*)" \
  "$BIN hash -a sha256 -j 1 many" "$BIN hash -a sha256 -j 4 many" "$BIN hash -a sha256 -j 16 many" \
  "b3sum --num-threads 1 -- $(echo many/*)" "$BIN hash -a blake3 -j 4 many" "$BIN hash -a blake3 -j 16 many" 2>/dev/null \
  | grep -E "Benchmark|Time" | sed 's/many\/f[0-9]* //g'

echo "## peak RSS"
for spec in "sha256 4 many" "blake3 4 many" "sha256 16 many" "blake3 1 big.bin"; do
  set -- $spec
  /usr/bin/time -f "%M" -o rss "$BIN" hash -a "$1" -j "$2" "$3" > /dev/null
  echo "$1 -j $2 $3: $(awk '{printf "%.1f MiB", $1/1024}' rss)"
done
