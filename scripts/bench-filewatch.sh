#!/usr/bin/env bash
# Event latency/loss and idle-cost benchmark for filewatch (Linux). Requires /usr/bin/time.
# Usage: scripts/bench-filewatch.sh [workdir]
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q -p filewatch --bin filewatch --example latency
BIN=$PWD/target/release/filewatch
WORK=${1:-$(mktemp -d)}
mkdir -p "$WORK"

echo "## environment"
echo "cpu: $(lscpu | sed -n 's/^Model name:[[:space:]]*//p') ($(nproc) logical)"
echo "kernel: $(uname -sr)"; rustc --version
echo "filesystem: $(df -T "$WORK" | awk 'NR==2{print $2}'); inotify max_queued_events: $(cat /proc/sys/fs/inotify/max_queued_events)"
echo "binary size: $(stat -c %s "$BIN") bytes"

echo "## delivery latency and loss (debounce 0)"
for spec in "1000 100" "5000 1000" "20000 5000"; do
  set -- $spec
  d="$WORK/lat-$1-$2"; mkdir -p "$d"
  target/release/examples/latency "$BIN" "$d" "$1" "$2"
done

echo "## idle cost: watching a 10,000-directory tree for 30 s with no changes"
t="$WORK/tree"
for a in $(seq 0 99); do for b in $(seq 0 99); do mkdir -p "$t/d$a/s$b"; done; done
/usr/bin/time -f "%M %U %S %e" -o "$WORK/idle" timeout -s INT 30 "$BIN" "$t" -q > /dev/null 2>&1 || true
tail -n 1 "$WORK/idle" | awk '{printf "idle: peak RSS %.1f MiB, CPU %.2f s over %.0f s (includes startup and registering 10,101 watches)\n", $1/1024, $2+$3, $4}'
