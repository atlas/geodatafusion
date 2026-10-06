#!/usr/bin/env bash
# H5: clean build time of geodatafusion before/after adding datafusion-macros + datafusion-doc.
# Usage: h5_build_times.sh <reps-full> <reps-crate>
# Expects scratch copies at $E5/repo-before and $E5/repo (see plans/experiments/e5-dependencies.md).
set -euo pipefail
E5=${E5:-/home/mikkel/Projects/geodatafusion/target/experiments/e5}
REPS_FULL=${1:-2}
REPS_CRATE=${2:-5}
export RUSTUP_TOOLCHAIN=1.97.1
OUT=$E5/h5_times.csv
[ -f "$OUT" ] || echo "kind,variant,profile,rep,seconds,load1_before,load1_after" > "$OUT"
load1() { cut -d' ' -f1 /proc/loadavg; }
run() { # kind variant profile rep cmd...
  local kind=$1 variant=$2 profile=$3 rep=$4; shift 4
  local lb; lb=$(load1)
  local t0; t0=$(date +%s.%N)
  "$@" >/dev/null 2>&1
  local t1; t1=$(date +%s.%N)
  echo "$kind,$variant,$profile,$rep,$(echo "$t1 - $t0" | bc),$lb,$(load1)" | tee -a "$OUT"
}
for rep in $(seq 1 "$REPS_FULL"); do
  for profile in debug release; do
    flag=""; [ "$profile" = release ] && flag="--release"
    for variant in before after; do
      dir=$E5/repo; [ "$variant" = before ] && dir=$E5/repo-before
      tgt=$E5/target-$variant-$profile
      rm -rf "$tgt"
      run full "$variant" "$profile" "$rep" env CARGO_TARGET_DIR="$tgt" cargo build --offline -q $flag -p geodatafusion --manifest-path "$dir/Cargo.toml"
    done
  done
done
# Crate-only: dependencies already built; rebuild only geodatafusion.
for rep in $(seq 1 "$REPS_CRATE"); do
  for profile in debug release; do
    flag=""; [ "$profile" = release ] && flag="--release"
    for variant in before after; do
      dir=$E5/repo; [ "$variant" = before ] && dir=$E5/repo-before
      tgt=$E5/target-$variant-$profile
      env CARGO_TARGET_DIR="$tgt" cargo clean --offline -q $flag -p geodatafusion --manifest-path "$dir/Cargo.toml"
      run crate "$variant" "$profile" "$rep" env CARGO_TARGET_DIR="$tgt" cargo build --offline -q $flag -p geodatafusion --manifest-path "$dir/Cargo.toml"
    done
  done
done
