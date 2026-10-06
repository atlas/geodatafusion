# PostGIS parity tests (sqllogictest)

These tests measure how closely geodatafusion matches PostGIS. Each `.slt` file holds SQL
queries, and the expected results were **recorded from a real PostGIS** (the oracle). The runner
executes the same queries on DataFusion with geodatafusion registered and compares the output.

```
tests/sqllogictests/
├── main.rs               runner: CLI, filtering, parity tracking
├── datafusion_engine.rs  geodatafusion engine (+ the `::geometry` literal shim)
├── postgis.rs            PostGIS engine (used to record and check expectations)
├── render.rs             engine-neutral value rendering
├── parity.txt            passing records per file; the ratchet CI enforces
└── slt/
    ├── postgis_docs/     GENERATED from the PostGIS reference docs examples, don't edit
    ├── geodatafusion/    hand-written edge-case tests (expected output still from PostGIS)
    └── smoke/            renderer sanity checks
```

## Running

`cargo slt` is a cargo alias (in `.cargo/config.toml`) for
`cargo test -p geodatafusion --all-features --test sqllogictests --`.

```bash
cargo slt                      # everything; fails if results differ from parity.txt
cargo slt st_area              # one function (exact file-stem match, otherwise substring)
cargo slt st_area st_length    # several
cargo slt geodatafusion/st_area  # one specific file
cargo slt -v                   # print every failure (default: only when <= 5 files selected)
cargo slt --list st_as         # list matching files
cargo slt --update-parity      # record current pass counts (run after improving parity)
```

These commands need PostGIS (`dev/postgis.sh start`, or set `POSTGIS_URL`):

```bash
cargo slt --complete geodatafusion/st_area  # (re)record expected output from PostGIS
cargo slt --postgis                         # check every expectation still holds on PostGIS
```

## How values are compared

Both engines render results through `render.rs`, so neither side's native text output gets
compared:

- **geometry/geography** is rendered as canonical EWKT (`SRID=4326;POINT(1 2)`), from PostGIS
  hex EWKB on one side and GeoArrow arrays on the other.
- **floats** (including numeric/decimal) are rounded to 12 significant digits.
- **box2d/box3d** render as `BOX(xmin ymin,xmax ymax)` / `BOX3D(...)`.
- **text**, including `geoarrow.wkt` columns returned by e.g. `ST_AsText`, is compared verbatim.
  Text formatting is part of the behaviour being tested.
- NULL renders as `NULL` and the empty string as `(empty)`.

Multi-row results without `ORDER BY` use `rowsort`.

## The `::geometry` shim

DataFusion has no `geometry` SQL type, so the geodatafusion engine rewrites literal casts
before planning: `'POINT(1 2)'::geometry` becomes `ST_GeomFromText(...)`, `'SRID=..'` literals
become `ST_GeomFromEWKT(...)`, hex becomes `ST_GeomFromEWKB(X'..')`, and `::geography` becomes
`ST_GeogFromText(...)`. This keeps the function tests meaningful. Real `geometry` type support
is a separate parity gap.

## parity.txt

`parity.txt` lists `passed/total` for every file. `cargo slt` fails if any number differs from
it, whether that's a regression or an unrecorded improvement. After making more tests pass,
run `cargo slt --update-parity` and commit the result. It is always recorded with
`--all-features`, matching CI.

## Regenerating the docs examples

```bash
dev/postgis.sh start
uv run dev/extract_postgis_doc_examples.py          # rewrites slt/postgis_docs/
cargo slt --complete postgis_docs                   # record expected output
cargo slt --update-parity
```

The PostGIS version of the docs (`--postgis-version`) should match the image in
`dev/postgis.sh`. The generated SQL is derived from the PostGIS documentation (CC BY-SA 3.0).
