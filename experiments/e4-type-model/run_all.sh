#!/usr/bin/env bash
# Reproduce every E4 result. Run from this directory.
set -euo pipefail
export CARGO_TARGET_DIR=/home/mikkel/Projects/geodatafusion/target/experiments/e4
export RUSTUP_TOOLCHAIN=1.97.1
B=$CARGO_TARGET_DIR/release
PY="uv run --no-project --with geodatafusion --with datafusion --with pyarrow --with geoarrow-pyarrow --with geopandas --with pyogrio --with duckdb --with pyproj python"

(cd rust && cargo build --release --bins)
(cd rust-geoarrow09 && cargo build --release)
mkdir -p out

# H2b
$B/h2b > h2b_results.md
(cd ../.. && cargo slt -v) > slt_verbose.txt 2>&1 || true

# H2c
python3 h2c_extract.py
$B/h2c repro > h2c_repro_0.8.md
$B/h2c h2c_literals.json > h2c_roundtrip_0.8.jsonl
$B/e4-geoarrow09 h2c_literals.json > h2c_repro_0.9.md

# H3b
rm -rf out/h3b && $B/h3b out/h3b > h3b_results.md
$PY h3b_export.py out/h3b > h3b_export_results.md 2>&1

# H9
rm -rf out/h9 out/h9_rust
$PY h9_producers.py out/h9 > h9_results.md 2>&1
$PY h9_geo_string_crs.py out/h9 > h9_geo_string_crs.md 2>&1
$B/h9 out/h9_rust out/h9 > h9_rust_results.md

# PostGIS reference
psql postgresql://postgres:postgres@localhost:54329/postgres -Atf postgis_reference.sql > postgis_reference.txt
$PY h9_abbrev_projjson.py out/h9 > h9_abbrev_projjson.md 2>&1
