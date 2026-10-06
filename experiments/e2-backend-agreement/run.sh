#!/usr/bin/env bash
# Reproduce E2 end to end. Needs podman, uv, cargo (RUSTUP_TOOLCHAIN=1.97.1), cmake and a C++
# compiler (for the bundled GEOS 3.14.1).
#
# PostGIS runs in a private container on port 54330, because ST_Relate crashes the backend on
# some EMPTY inputs (see plans/experiments/e2-backend-agreement.md); don't point E2_DSN at a
# shared server.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
export E2_DATA=${E2_DATA:-$REPO/target/experiments/e2}
export CARGO_TARGET_DIR=$E2_DATA
mkdir -p "$E2_DATA/ne"

# Natural Earth layers (GeoJSON from nvkelso/natural-earth-vector).
for f in ne_110m_admin_0_countries ne_50m_admin_0_countries ne_50m_admin_1_states_provinces \
    ne_50m_lakes ne_50m_rivers_lake_centerlines ne_110m_populated_places ne_110m_coastline; do
  [ -s "$E2_DATA/ne/$f.geojson" ] || curl -sfL -o "$E2_DATA/ne/$f.geojson" \
    "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/$f.geojson"
done

# Private PostGIS 3.6.4 / GEOS 3.14.1 (same image as the parity oracle).
if ! podman container exists e2-postgis; then
  podman run -d --name e2-postgis -e POSTGRES_PASSWORD=postgres -p 127.0.0.1:54330:5432 \
    docker.io/postgis/postgis:18-3.6
  until psql postgresql://postgres:postgres@localhost:54330/postgres -Atc 'select postgis_version()' >/dev/null 2>&1; do sleep 2; done
fi

cd "$HERE"
uv run -q --with 'psycopg[binary]' --with pyarrow python gen_corpus.py
uv run -q --with 'psycopg[binary]' python pg_eval.py
RUSTUP_TOOLCHAIN=1.97.1 cargo run --release
python3 summarize.py --examples > "$E2_DATA/out/tables.md"
echo "Results: $E2_DATA/out/tables.md"
# Clean up: podman rm -f e2-postgis
