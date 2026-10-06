#!/usr/bin/env bash
# Recreate the E3 workspace copy (target/experiments/e3/ws): the repo without target/.git, plus
# ws.patch (the plan's `geos-static` feature; a Python `geos` module with ST_LineMerge and a
# `geos-wide` probe), with geos-sys 2.0.9 / geos-src 0.2.4 (GEOS 3.14.1) pinned in both lockfiles.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
W="${E3_WS:-$REPO/target/experiments/e3/ws}"
export RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-1.97.1}
rm -rf "$W" && mkdir -p "$W"
rsync -a --exclude=/target --exclude=/.git --exclude=/experiments "$REPO"/ "$W"/
(cd "$W" && patch -p1 < "$REPO/experiments/e3-geos/ws.patch")
for d in "$W" "$W/python"; do
  (cd "$d" && cargo metadata -q --format-version 1 > /dev/null \
    && cargo update -q -p geos-sys --precise 2.0.9 && cargo update -q -p geos-src --precise 0.2.4)
done
grep -A1 'name = "geos-s' "$W/Cargo.lock" "$W/python/Cargo.lock"
