#!/usr/bin/env bash
# End-to-end benchmark harness for disklens (Linux). Requires: hyperfine, /usr/bin/time, du.
# Cold-cache runs need passwordless sudo for `sysctl vm.drop_caches=3`; they are skipped otherwise.
# Usage: scripts/bench-disklens.sh [workdir]
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q -p disklens
BIN=$PWD/target/release/disklens
WORK=${1:-$(mktemp -d)}; mkdir -p "$WORK"; trap 'rm -rf "$WORK"' EXIT
T="$WORK/tree"
echo "## environment"
echo "cpu: $(lscpu | sed -n 's/^Model name:[[:space:]]*//p') ($(nproc) logical)"
echo "kernel: $(uname -sr)"; rustc --version; du --version | head -1
echo "binary size: $(stat -c %s "$BIN") bytes"
echo "filesystem: $(df -T "$WORK" | awk 'NR==2{print $2}')"

# 100,000 files: 100 top dirs x 10 subdirs x 100 files of 0-4 KiB.
python3 - "$T" <<'PY'
import os, sys, random
root = sys.argv[1]; random.seed(42)
for a in range(100):
    for b in range(10):
        d = os.path.join(root, f"d{a:03}", f"s{b}"); os.makedirs(d, exist_ok=True)
        for c in range(100):
            with open(os.path.join(d, f"f{c:03}"), "wb") as f: f.write(b"x" * random.randint(0, 4096))
PY
echo "fixture: $(find "$T" -type f | wc -l) files, $(find "$T" -type d | wc -l) directories"
du -s "$T" > /dev/null

echo "## correctness vs du (bytes)"
echo "du allocated/apparent:       $(du -s -B1 "$T" | cut -f1) / $(du -s -b "$T" | cut -f1)"
echo "disklens allocated/apparent: $($BIN "$T" --json -d 0 -t 0 | python3 -c 'import json,sys;r=json.load(sys.stdin)["root"];print(r["allocated"],"/",r["logical"])')"

echo "## warm cache, 100k files"
hyperfine -N --warmup 2 --runs 10 "du -s $T" "$BIN $T -j 1" "$BIN $T -j 4" "$BIN $T" 2>/dev/null | grep -E "Benchmark|Time" | sed "s#$BIN#disklens#; s#$T#<tree>#"

echo "## warm cache, /usr (real-world tree)"
echo "/usr: $(find /usr -xdev -type f 2>/dev/null | wc -l) files"
hyperfine -N --warmup 2 --runs 10 -i "du -sx /usr" "$BIN /usr -x" 2>/dev/null | grep -E "Benchmark|Time" | sed "s#$BIN#disklens#"

if sudo -n true 2>/dev/null; then
  echo "## cold cache (drop_caches before each run), 100k files"
  hyperfine -N --runs 5 --prepare "sudo -n sysctl -q vm.drop_caches=3" "du -s $T" "$BIN $T" 2>/dev/null | grep -E "Benchmark|Time" | sed "s#$BIN#disklens#; s#$T#<tree>#"
fi

echo "## peak RSS"
for target in "$T" /usr; do
  /usr/bin/time -f "%M" -o "$WORK/rss" "$BIN" "$target" -x > /dev/null 2>&1 || true
  echo "$target: $(awk '{printf "%.1f MiB", $1/1024}' "$WORK/rss")" | sed "s#$T#<tree>#"
done
