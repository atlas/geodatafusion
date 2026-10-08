# PostGIS parity tests (sqllogictest)

These tests measure how closely geodatafusion matches PostGIS. Each `.slt` file holds SQL
queries, and the expected results were **recorded from a real PostGIS** (the oracle). The runner
executes the same queries on DataFusion with geodatafusion registered and compares the output.

```
tests/sqllogictests/
├── main.rs               runner: CLI, filtering, parity tracking
├── datafusion_engine.rs  geodatafusion engine
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

## Library versions

The expected results were recorded with PostGIS 3.6.4 and GEOS 3.14.1 (`dev/postgis.sh`). GEOS
output changes between minor versions (buffer vertices, Voronoi order, MakeValid), so
`--all-features` builds the same GEOS from source through the `geos-static` feature
(`geos-src` 0.2.4, pinned in `Cargo.lock`). Don't record or update parity with a system GEOS.
When the PostGIS image moves to a new GEOS, update `geos-src` and `geos-sys`, re-record
(`cargo slt --complete`) and update `parity.txt` together.

## How values are compared

Both engines render results through `render.rs`, so neither side's native text output gets
compared:

- **geometry/geography** is rendered as canonical EWKT (`SRID=4326;POINT(1 2)`), from PostGIS
  hex EWKB on one side and GeoArrow arrays on the other.
- **floats** (including numeric/decimal) are rounded to 12 significant digits.
- **box2d/box3d** render as `BOX(xmin ymin,xmax ymax)` / `BOX3D(...)`.
- **text**, such as the output of `ST_AsText`, is compared verbatim.
  Text formatting is part of the behaviour being tested.
- NULL renders as `NULL` and the empty string as `(empty)`.

Multi-row results without `ORDER BY` use `rowsort`.

## The geometry type and the dialect

The geodatafusion engine builds its session with `GeoTypePlanner` and `register`, so
`'POINT(1 2)'::geometry`, `geometry(Point, 4326)` columns and `::text` casts plan as they would for
a user. Until DataFusion 55 the engine rewrote literal casts into constructor calls instead. It
parses in the PostgreSQL dialect, which PostGIS's operators need and in which `^` is a power, as in
PostgreSQL, rather than DataFusion's XOR.

## Aggregates under other names

Where geodatafusion implements both a scalar and an aggregate form of a PostGIS function, the
scalar takes the PostGIS name and the aggregate is `st_<name>_agg` (plans D8): `ST_Collect_Agg`
is PostGIS's aggregate `ST_Collect(geometry)`. So that their tests can be recorded, the PostGIS
engine defines the same names at the start of every file (`AGGREGATE_ALIASES` in `postgis.rs`),
built from PostGIS's own functions and rolled back with the rest of the file's transaction.

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
