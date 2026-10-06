#!/usr/bin/env bash
# H12: clean bundled-PROJ build of proj-check in ubuntu:24.04, timed, then run the comparison.
# Usage: run_container.sh [jobs] [profile]   (jobs defaults to 4, the vCPU count of GitHub's ubuntu-24.04 runner; profile release or dev)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-/home/mikkel/Projects/geodatafusion/target/experiments/e5-h12-container}
JOBS=${1:-4}
PROFILE=${2:-release}
mkdir -p "$OUT/cargo-home" "$OUT/target"
podman build -q -t e5-ubuntu -f "$HERE/Containerfile" "$HERE" >/dev/null
podman run --rm --cpus "$JOBS" \
  -v "$HERE:/src:ro,Z" -v "$OUT:/out:Z" \
  -e CARGO_HOME=/out/cargo-home -e CARGO_TARGET_DIR=/out/target -e CARGO_BUILD_JOBS="$JOBS" -e PROFILE="$PROFILE" \
  e5-ubuntu bash -c '
    set -euo pipefail
    cd /src/proj-check
    D=$PROFILE; [ "$PROFILE" = dev ] && D=debug
    cargo fetch -q
    rm -rf /out/target
    echo "load before: $(cat /proc/loadavg)"
    /usr/bin/time -f "$PROFILE build wall=%e s user=%U s sys=%S s maxrss=%M KB" cargo build --offline --profile $PROFILE 2>&1 | grep -E "wall=|error|warning: proj-sys" || true
    echo "load after: $(cat /proc/loadavg)"
    ls -l /out/target/$D/proj-check /out/target/$D/probe /out/target/$D/hello
    for b in proj-check probe hello; do strip -o /out/$b.stripped /out/target/$D/$b; done
    ls -l /out/*.stripped
    find /out/target/$D/build -name "libproj.a" -exec ls -l {} \;
    find /out/target/$D/build -name proj.db -exec ls -l {} \;
    /out/target/$D/proj-check /src/cases.tsv /out/results-container.tsv
  '
