#!/usr/bin/env bash
# H7b: clean-build time with system GEOS vs bundled static GEOS (geos-src 0.2.4 = 3.14.1).
# Runs inside the e3-ubuntu container (see in-ubuntu.sh), against the workspace copy that has the
# plan's `geos-static` feature (target/experiments/e3/ws). Mirrors CI's `cargo test --all-features`
# compile step. Usage (host): ./in-ubuntu.sh taskset -c 0-3 experiments/e3-geos/h7b.sh <label> <variant...>
set -euo pipefail
LABEL="$1"; shift
WS=/work/target/experiments/e3/ws
OUT=/work/target/experiments/e3/h7b
mkdir -p "$OUT"
for variant in "$@"; do
  case "$variant" in
    system) FEAT="geodatafusion/geos-3_11" ;;
    static) FEAT="geodatafusion/geos-static" ;;
    none) FEAT="" ;;
  esac
  TD="/work/target/experiments/e3-h7b-$LABEL-$variant"
  rm -rf "$TD"
  start=$(date +%s.%N); load0=$(cut -d' ' -f1-3 /proc/loadavg)
  (cd "$WS" && CARGO_TARGET_DIR="$TD" cargo test --workspace --no-run --offline ${FEAT:+--features $FEAT} --timings) > "$OUT/$LABEL-$variant.log" 2>&1
  end=$(date +%s.%N); load1=$(cut -d' ' -f1-3 /proc/loadavg)
  secs=$(python3 -c "print(round($end - $start, 1))")
  echo -e "$LABEL\t$variant\tnproc=$(nproc)\twall_s=$secs\tload_before=$load0\tload_after=$load1\t$(date -Is)" | tee -a "$OUT/results.tsv"
  cp "$TD"/cargo-timings/cargo-timing.html "$OUT/$LABEL-$variant-timing.html" 2>/dev/null || true
done
