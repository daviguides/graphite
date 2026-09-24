#!/usr/bin/env bash
# Run every engine/backend/size in its own process so a crash (abort,
# segfault) in one engine is recorded instead of killing the whole run.
# usage: ./run.sh [size ...]      (default: small medium)
set -u
cd "$(dirname "$0")"
export LBUG_VERSION=0.20.4
cargo build --release --workspace >/dev/null 2>&1 || { echo "build failed"; exit 1; }
mkdir -p results .data
sizes=("${@:-small medium}")
[ $# -eq 0 ] && sizes=(small medium)
runs=(
  "cozo-bench mem" "cozo-bench sqlite" "cozo-bench newrocksdb"
  "lbug-bench disk" "lbug-bench mem"
  "diy-bench redb"
)
for size in "${sizes[@]}"; do
  for r in "${runs[@]}"; do
    set -- $r
    bin=$1; backend=$2
    log=".data/run-$bin-$backend-$size.log"
    start=$(date +%s)
    ./target/release/$bin --size "$size" --backend "$backend" >"$log" 2>&1
    code=$?
    echo "$(date -u +%FT%TZ) $bin $backend $size exit=$code secs=$(( $(date +%s) - start ))" | tee -a results/exit-codes.log
  done
done
