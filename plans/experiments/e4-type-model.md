# E4: type model

Experiment for decisions D2 (output encoding), D3 (plain text/binary outputs) and D9 (CRS
representation). Hypotheses and decision rules are quoted from [hypotheses.md](../hypotheses.md)
and were not changed. Code, scripts and raw outputs are in
[`experiments/e4-type-model/`](../../experiments/e4-type-model/).

## Hypotheses

- **H2b (D2).** Native outputs break common SQL that WKB outputs don't: `UNION ALL`, `CASE`,
  `COALESCE`, `IN (subquery)`, `array_agg`, joins and comparisons across results of different
  geometry functions, and mixed with a WKB column. Rule: if more than one construct in the
  matrix fails with native outputs and works with WKB, that counts against native outputs in D2.
- **H2c (D2).** The geoarrow-rs `GeometryCollection` collapse and mixed-dimension bugs affect
  doc-test records with native `GeometryArray` outputs, and none with WKB. Rule: if they affect
  any records, either use WKB for those outputs or fix them upstream first.
- **H3b (D3).** GeoArrow-tagged `Utf8`/`Binary` outputs (today's `ST_AsText`/`ST_AsBinary`) break
  DataFusion string and binary functions, or export to Parquet or Python. Rule: if anything
  breaks, plain types are a bug fix, not a matter of taste.
- **H9 (D9).** Real GeoParquet and GeoArrow producers mostly write the WGS 84 CRS as PROJJSON or
  `OGC:CRS84`, not `EPSG:4326`. Rule: `util::srid` must map whatever the producers write to 4326.
  Write the form that round-trips through the most tools.

## Verdicts

| Hypothesis | Verdict under the rule | Key data |
|---|---|---|
| H2b | **Holds. Counts against native outputs in D2.** | Every construct tested (9 of 9) fails with native outputs and works with WKB for each pair of differing native types. Even when both sides have the same native type, 4 constructs fail (CASE, COALESCE, make_array, array_agg), because DataFusion 54 drops extension metadata there. In the parity suite, 1 current failure has this cause, and 2 more are hidden behind missing functions. |
| H2c | **Holds. Use WKB for those outputs or fix upstream first.** | 9 doc-test records (of 577 parsed) contain a one-member GEOMETRYCOLLECTION that geoarrow-array's `GeometryBuilder` collapses: 5 in inputs, 4 in expected outputs. WKB: 0. 0 records have mixed dimensions. Both bugs are still in geoarrow-array 0.9.0 (latest) and on geoarrow-rs `main`. |
| H3b | **Holds (export/Python); no DataFusion function breaks. Plain types are a bug fix.** | None of the 51 DataFusion queries fails because of the tag; failures match the untagged control. But the tag spreads to values that aren't geometries (UNION with a literal, CAST to VARCHAR). In Python, the tagged ST_AsText column breaks `pyarrow.compute` string kernels, pandas `.str`, and `GeoDataFrame.from_arrow` on the whole table. |
| H9 | **Holds. Map PROJJSON, `OGC:CRS84` and a missing GeoParquet `crs` to 4326. Write PROJJSON (full, not abbreviated).** | No external producer writes `EPSG:4326` as an authority code. GeoPandas, DuckDB, pyogrio and geoarrow-pyarrow (given a pyproj CRS) write PROJJSON. GDAL and DuckDB omit the GeoParquet `crs` for WGS 84 (which means `OGC:CRS84`). For an Arrow hand-off, an `EPSG:4326` tag round-trips through GeoPandas, DuckDB and GDAL. In GeoParquet, though, only full PROJJSON survives every tool: geoarrow-rs's writer silently drops non-PROJJSON CRSs, and DuckDB rejects string CRSs. |

## Environment

- Repository at `d2eb234` plus the uncommitted working-tree changes listed in the session's git
  status. geodatafusion was built with `--features geos-3_11`.
- rustc 1.97.1. The experiment crate resolved **DataFusion 54.1.0**; the workspace lock pins 54.0.0,
  but 54.0.0 with the experiment's extra features (`parquet`) could not be locked alongside the
  workspace lockfile, so I used the patch release. arrow 58, geoarrow-array/-schema 0.8.0,
  geoparquet 0.8.0, wkt 0.14.0, wkb 0.9.2. A second crate uses geoarrow-array 0.9.0 (arrow 59).
- Python (via `uv run`): pyarrow 25.0.1, geoarrow-pyarrow 0.3.0, geopandas 1.2.0, shapely 2.1.2,
  pyproj 3.8.0, pyogrio 0.13.0 (bundled GDAL 3.12.4), duckdb 1.5.6 with `spatial`. From PyPI:
  geodatafusion 0.3.1, the latest release, which pins `datafusion<54`, with datafusion 53.0.0.
- System GDAL 3.13.3 (`ogr2ogr`/`ogrinfo`, with the Parquet and Arrow drivers).
- PostGIS 3.6.4, PostgreSQL 18.6, GEOS 3.14.1 at `localhost:54329`. Used only as a reference for
  what the constructs return (`postgis_reference.txt`).

## H2b: SQL constructs × pairs of geometry inputs

### Method

`rust/src/bin/h2b.rs` registers geodatafusion and builds a two-row `MemTable` `t`:

- `id`: 1, 2.
- `wkb`: `geoarrow.wkb` without a CRS: `POINT(1 2)`, `LINESTRING(0 0,2 4)`. The centroid of
  both rows is `POINT(1 2)`.
- `wkb4326`: the same values, with the CRS `EPSG:4326`.
- `pt_sep`, `pt_il`, `pt_z`: native points (XY separated, XY interleaved, XYZ).

Source expressions and the types geodatafusion gives them today:

| key | expression | native type | "all WKB" type |
|---|---|---|---|
| P | `ST_Centroid(wkb)` | `Struct<x,y>` geoarrow.point | `Binary` geoarrow.wkb |
| G | `ST_GeomFromText(ST_AsText(wkb))` | `Union` geoarrow.geometry | `Binary` geoarrow.wkb |
| W | `wkb` | `Binary` geoarrow.wkb | `Binary` geoarrow.wkb |
| PS | `pt_sep` | `Struct<x,y>` geoarrow.point | `Binary` geoarrow.wkb |
| PI | `pt_il` | `FixedSizeList[2]` geoarrow.point | `Binary` geoarrow.wkb |
| PZ | `pt_z` | `Struct<x,y,z>` geoarrow.point | `Binary` geoarrow.wkb |
| W4326 | `wkb4326` | `Binary` geoarrow.wkb + EPSG:4326 | same |

"All WKB" is simulated by wrapping every source in `ST_AsBinary(..)`, which returns `Binary`
tagged `geoarrow.wkb` with the input CRS, the same as a WKB-returning function would.

Each construct ends in `ST_AsText(..)`, so the result must still be usable as a geometry. The
constructs:

- `UNION ALL` of two subqueries.
- `CASE WHEN id = 1 THEN a ELSE b END`.
- `COALESCE(a, b)`.
- A 2-row `VALUES` with constant versions of the sources. PI has no constant constructor.
- `a IN (SELECT b ...)`.
- `unnest(make_array(a, b))`.
- `unnest(array_agg(..))` over a `UNION ALL`.
- `JOIN ... ON a = b`.
- `a = b` in the projection.

The exact SQL is listed at the end of `h2b_results.md`.

### Results (native / all-WKB; FAIL = error)

| pair | UNION ALL | CASE | COALESCE | VALUES | IN (subquery) | make_array | array_agg | JOIN ON = | = |
|---|---|---|---|---|---|---|---|---|---|
| P-PS (same type, control) | ok / ok | FAIL / ok | FAIL / ok | ok / ok | ok / ok | FAIL / ok | FAIL / ok | ok / ok | ok / ok |
| P-G (Point vs Geometry) | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-W (native Point vs WKB column) | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| G-W (native Geometry vs WKB column) | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-PI (separated vs interleaved) | FAIL / ok | FAIL / ok | FAIL / ok | n/a | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| P-PZ (XY vs XYZ) | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok | FAIL / ok |
| W-W4326 (same storage, different CRS) | ok / ok | ok / ok | ok / ok | FAIL / FAIL | ok / ok | ok / ok | ok / ok | ok / ok | ok / ok |

Typical errors (P vs W):

- UNION ALL: `Incompatible inputs for Union: Previous inputs were of type Struct("x"..., "y"...), but got incompatible type Binary`.
- CASE: `Failed to coerce then (Struct(...)) and else (Binary) to common types in CASE WHEN expression`.
- COALESCE: `Function 'coalesce' user-defined coercion failed ... Expect to get struct but got Binary`.
- VALUES: `Inconsistent metadata across values list ... "geoarrow.point" but found ... "geoarrow.wkb"`.
- IN: `expr type Struct(...) can't cast to Binary in InSubquery`.
- JOIN and `=`: `Cannot infer common argument type for comparison operation Struct(...) = Binary`.

P vs PZ and P vs PI fail in the same way, on mismatched struct keys or `Struct` vs
`FixedSizeList`.

**Same-type controls** (one source with itself):

- For P (`geoarrow.point`), CASE, COALESCE, make_array and array_agg fail with
  `Invalid argument error: Extension type name missing`.
- For G (`geoarrow.geometry`), the same four fail with `Data not conforming to GeoArrow
  specification: Only FixedSizeList, Struct, Binary, ... are unambiguously typed`.

DataFusion 54 drops the field's extension metadata in these four constructs. A native array
without its metadata can't be read as a geometry. A WKB array without metadata is plain `Binary`,
which every geodatafusion function accepts as WKB, so the WKB mode passes. **The WKB passes in
these four constructs lose the CRS**: with `wkb4326` alone, the `ST_AsText` output after CASE,
COALESCE, make_array or array_agg has no CRS. UNION ALL and VALUES keep it. For W vs W4326,
UNION ALL silently takes the first branch's CRS (none); VALUES rejects the CRS mismatch in both
modes. PostGIS keeps per-value SRIDs, so `'SRID=4326;POINT(1 2)' UNION ALL 'POINT(1 2)'` returns
both SRIDs.

Values were checked by hand. The WKB results match the PostGIS reference queries:

- JOIN ON P = W: (1,1), (2,1).
- `=` (P vs W): true, false.
- P = PZ: false in WKB mode, and PostGIS also returns false for POINT vs POINT Z.

The PostGIS reference queries are in `postgis_reference.sql` and `postgis_reference.txt`. All the
mixed constructs work in PostGIS.

### Parity-suite scan

`cargo slt -v` gives 55/582 records passing, the same as `parity.txt`. Only the first error of
each record is visible, and 431 records fail first on a missing function. Failures caused by
the type model:

- **Directly visible (1):** `st_geohash.slt:18`, `ST_GeoHash('LINESTRING(...)'::geometry)`. The
  shim yields `Union` (geoarrow.geometry), and `st_geohash`'s Point-only signature rejects it.
  This is a native-type-specific signature failure.
- **VALUES metadata (1):** `st_asgeojson.slt:8`. Row 1 is `'POINT(1 1)'::geometry`, and rows 2-3
  are untyped strings: `Inconsistent metadata across values list`. It mixes a typed and an
  untyped row, so WKB outputs wouldn't obviously fix it, and `json_build_object` is missing anyway.
  I don't count it.
- **Hidden behind missing functions (2, maybe 3):** `st_contains.slt:20` and
  `st_containsproperly.slt:20` build `VALUES (ST_Buffer(...)), (ST_MakeLine(...)), (ST_Point(1,1))`.
  These are three different native output types, which fail exactly like the matrix's VALUES row.
  `st_makepolygon.slt:34` (`ARRAY[ST_Translate(..), ST_ExteriorRing(..)]`) fails if the two
  functions get different native types.
- Static scan of the 577 records: 0 use UNION, CASE, COALESCE or `IN (SELECT`. 28 use multi-row
  VALUES, 20 JOIN, 5 `ARRAY[`, 2 `array_agg`. Apart from the three above, these combine
  geometries of a single producer (`'...'::geometry` everywhere).

The doc-test suite rarely mixes producers, so in parity terms this is a small effect (1-3
records). The matrix, not the suite, carries H2b.

### Verdict

More than one construct (9 of 9) fails with native outputs and works with WKB, so H2b holds
and counts against native outputs in D2. Two caveats:

- DataFusion 54 drops metadata in CASE/COALESCE/make_array/array_agg, so WKB "works" there only
  by losing the GeoArrow type and CRS. With native outputs these constructs don't work at all,
  even for one output type.
- That is broader than G6 R5's "UNION/CASE/COALESCE over a WKB and a native geometry fail ...
  unless one side is cast". With today's native outputs, `CASE WHEN .. THEN ST_Centroid(a) ELSE
  ST_Centroid(b) END` already fails.

## H2c: GeometryCollection collapse and mixed dimensions

### Method

- `h2c_extract.py` extracts every geometry literal from the 577 parsed doc-test records: the
  quoted SQL strings and the geometry values in the expected output. That gives 1231 literals
  from 541 records.
- 216 literals were normalized for the `wkt` crate. It rejects EWKT's implicit Z/ZM (`POINT(1 2 3)`)
  and `POINTM` without a space.
- `rust/src/bin/h2c.rs` parses each literal and round-trips it through geoarrow-array's
  `GeometryBuilder` (the native output path; `ST_GeomFromText` and the `::geometry` shim use it)
  and through `WkbBuilder`. It then compares the WKT.
- `rust-geoarrow09` repeats this with geoarrow-array 0.9.0.
- 73 literals don't parse: 71 curves/surfaces, plus 2 GEOMETRYCOLLECTIONs that contain curves.

### Results

| | geoarrow-array 0.8.0 | geoarrow-array 0.9.0 |
|---|---|---|
| literals parsed | 1158 | 1158 |
| changed by `GeometryBuilder` | 9 (all one-member GEOMETRYCOLLECTIONs) | 9 (same) |
| changed by `WkbBuilder` | 0 | 0 |

Affected records (9):

| record | where | literal → after native round trip |
|---|---|---|
| st_collectionextract.slt:15 | input | `GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(0 0)))` → `POINT(0 0)` |
| st_collectionextract.slt:23 | input | nested one-member GC inside a two-member GC is flattened |
| st_collectionhomogenize.slt:8 | input | `GEOMETRYCOLLECTION(POINT(0 0))` → `POINT(0 0)` |
| st_collectionhomogenize.slt:14 | input | `GEOMETRYCOLLECTION(MULTIPOINT((0 0)))` → `MULTIPOINT((0 0))` |
| st_collectionhomogenize.slt:26 | input | nested one-member GC flattened |
| st_clusterintersecting.slt:8 | expected | `GEOMETRYCOLLECTION(LINESTRING(6 6,7 7))` → `LINESTRING(6 6,7 7)` |
| st_clusterwithin.slt:8 | expected | same |
| st_force_collection.slt:8 | expected | `GEOMETRYCOLLECTION Z(POLYGON Z(...))` → `POLYGON Z(...)` |
| st_split.slt:25 | expected | `GEOMETRYCOLLECTION(LINESTRING(0 0,100 100))` → `LINESTRING(...)` |

`st_force_collection.slt:13` is a tenth one-member GEOMETRYCOLLECTION, but it contains a
CIRCULARSTRING, which is out of scope. All 9 records fail today for other reasons (the functions
aren't implemented), so the bug is latent: it caps these records once the functions exist. In
the input cases the collapse happens in the `::geometry` → `ST_GeomFromText` path regardless of
the function under test.

**Mixed dimensions:** 0 doc-test records. No literal mixes coordinate dimensions, and PostGIS
rejects such geometries.

### Minimal reproductions (geoarrow-array 0.8.0; 0.9.0 is identical)

| input | `GeometryBuilder::push_geometry` | `WkbBuilder` |
|---|---|---|
| `GEOMETRYCOLLECTION(POINT(1 2))` | returns `POINT(1 2)` | unchanged |
| `GEOMETRYCOLLECTION Z(POINT Z(1 2 3))` | returns `POINT Z(1 2 3)` | unchanged |
| `GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(1 2)))` | returns `POINT(1 2)` | unchanged |
| `GEOMETRYCOLLECTION(POINT(1 2),POINT(3 4))` | unchanged | unchanged |
| WKB `GEOMETRYCOLLECTION(POINT Z(1 2 3), POINT(4 5))` | panic `builder/point.rs:99`: `coord dimension must be XY for this buffer; got Xyz.` | no panic (WKB copied) |
| WKB `GEOMETRYCOLLECTION Z(POINT Z(1 2 3), POINT(4 5))` | panic: `coord dimension must be XYZ ...; got Xy.` | no panic |
| `MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))` | panic `builder/multipolygon.rs:188` (`Option::unwrap()` on `None`) | no panic in the builder |

End to end through geodatafusion (DataFusion 54):

- `ST_AsText(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))'))` returns `POINT(1 2)`, and
  `ST_GeometryType` returns `ST_Point`. PostGIS returns `GEOMETRYCOLLECTION(POINT(1 2))`.
- `ST_GeomFromWKB(<mixed-dimension WKB>)` panics the task (`from_wkb` → `PointBuilder::push_point`).
- `ST_GeomFromText('MULTIPOLYGON(EMPTY,...)')` panics (`from_wkt` → `MultiPolygonBuilder`).
- `ST_AsText` of the same mixed-dimension WKB as plain `Binary` (the WKB path) works:
  `GEOMETRYCOLLECTION(POINT Z(1 2 3),POINT(4 5))`.

**Upstream status:** geoarrow-rs `main` still has the `if gc.num_geometries() == 1` collapse in
`rust/geoarrow-array/src/builder/geometry.rs:642`. The last commit to the file is from
2025-10-14, and no matching issue was found.

Side findings in `wkt` 0.14: the writer panics on a MULTIPOLYGON with an EMPTY member
(`to_wkt/geo_trait_impl.rs:242`). The parser rejects implicit Z/ZM and `MULTIPOINT(EMPTY,(1 2))`.

### Verdict

The collapse affects 9 records with native `GeometryArray` outputs and 0 with WKB. H2c holds:
either use WKB for those outputs (any geometry function that can return a collection, and the
`ST_GeomFromText`/`::geometry` input path), or fix upstream first. There is no local workaround
for the native builder, because `push_geometry_collection` is private. Mixed dimensions affect
no doc-test records, but they turn bad WKB input into a panic instead of an error.

## H3b: tagged ST_AsText/ST_AsBinary outputs

### Method

`rust/src/bin/h3b.rs` runs 51 queries over `ST_AsText(wkb4326)` (`Utf8` tagged `geoarrow.wkt`
with EPSG:4326) and `ST_AsBinary(wkb4326)` (`Binary`, `geoarrow.wkb`). Each query also runs on an
untagged control: `arrow_cast(.., 'Utf8'/'Binary')`, which drops the metadata. Both the result
and the output field's tag are recorded.

It then writes Parquet with `COPY ... TO` and `DataFrame::write_parquet`. `h3b_export.py` reads
those files with pyarrow (with and without geoarrow-pyarrow), GeoPandas and DuckDB, and runs
the same functions through the published geodatafusion 0.3.1 Python package.

### Results in DataFusion

- **Same result tagged and untagged:** `||` (both sides), `concat`, `concat_ws`, `LIKE`, `ILIKE`,
  `~`, `length`, `char_length`, `upper`, `lower`, `substr`, `replace`, `split_part`,
  `starts_with`, `trim`, `md5` (text and binary), `sha256`, `encode(.., 'hex'/'base64')`,
  `=`/`<`/`IN` against text, `=` between binaries, CASE/COALESCE with a literal, GROUP BY
  (text and binary), DISTINCT, ORDER BY, `string_agg`, `min`/`max`, `array_agg`, CAST and
  `arrow_cast`, and joins on text.
- **Fail identically tagged and untagged** (DataFusion behaviour, not the tag): `octet_length(binary)`
  (`requires String, but received Binary`), `length(binary)` (non-UTF-8), `substr(binary, ..)`,
  `CAST(binary AS VARCHAR)`.
- **The tag spreads to values that aren't geometries:**
  - `SELECT ST_AsText(g) ... UNION ALL SELECT 'abc'` returns a column tagged `geoarrow.wkt` that
    contains `abc`.
  - `UNION ALL SELECT X'00'` returns a `geoarrow.wkb` column that contains `00`.
  - `CAST(ST_AsText(g) AS VARCHAR)` returns `Utf8View` tagged `geoarrow.wkt`.
  - `CAST(ST_AsBinary(g) AS VARCHAR)` returns `Utf8View` tagged **`geoarrow.wkb`**, an invalid
    storage type for that extension. This is DataFusion 54's cast metadata copy (G6 §1.3 gap 1).
  - String functions (`||`, `upper`, ...) drop the tag correctly.
- Side finding: `ST_AsText` of a plain `Utf8` returns the string unparsed. `ST_AsText('POINT(1 2) junk')`
  returns `POINT(1 2) junk`.

### Results on export and in Python

- The DataFusion-written Parquet keeps the tags in its Arrow schema (`wkt: string
  [geoarrow.wkt {"crs":"EPSG:4326",...}]`, `wkb: binary [geoarrow.wkb ...]`), with no `geo`
  metadata.
- **pyarrow without geoarrow-pyarrow:** plain `string`/`binary`; everything works.
- **pyarrow with geoarrow-pyarrow imported** (the usual state in a GeoPandas session):
  - The ST_AsText column becomes `extension<geoarrow.wkt>`. `pc.utf8_upper` and `pc.utf8_length`
    fail (`no kernel matching input types (extension<geoarrow.wkt<WktType>>)`). pandas
    `.str.upper()` fails the same way. The plain-string control works.
  - The UNION-with-literal file reads, but `ga.as_wkt` fails (`Expected geometry type at byte 0`).
  - The `CAST(ST_AsBinary AS VARCHAR)` file can't be read at all (`Can't interpret string_view as
    geoarrow.wkb`), and neither can a 0-row file. GeoPandas and DuckDB-from-Arrow fail on it the
    same way.
- **GeoPandas:** `read_parquet` fails for every DataFusion-written file (`Missing geo metadata`).
  This happens with or without the tag; DataFusion writes no GeoParquet.
  - `GeoDataFrame.from_arrow(table)` **fails on the whole table** (`Unknown GeoArrow extension type:
    geoarrow.wkt`) because of the ST_AsText column, although a valid `geoarrow.wkb` column is
    present.
  - With that column dropped, it works: geometry `wkb`, CRS EPSG:4326.
- **DuckDB 1.5.6:** `read_parquet` ignores the Arrow-schema tags (`wkt` VARCHAR, `wkb` BLOB). From
  Arrow, `geoarrow.wkb` becomes `GEOMETRY('EPSG:4326')`, and `geoarrow.wkt` stays VARCHAR.
- **Published geodatafusion 0.3.1 + datafusion 53:** `ST_AsText` and `ST_AsBinary` reach Python as
  `extension<geoarrow.wkt>`/`extension<geoarrow.wkb>` (with geoarrow-pyarrow imported), so the
  failures above apply to direct hand-off too. `upper(..)`, `||`, `md5`, `encode`, `LIKE` inside
  DataFusion work.

### Verdict

No DataFusion string or binary function breaks because of the tag. Export and the Python hand-off
do break, though, and the tag spreads to arbitrary strings and onto invalid storage. Under the
rule ("if anything breaks"), H3b holds: plain `Utf8`/`Binary` for ST_AsText/ST_AsBinary is a bug
fix.

## H9: CRS representations

### What producers write (WGS 84 and projected)

| producer | EPSG:4326 | OGC:CRS84 | EPSG:3857 / EPSG:2263 |
|---|---|---|---|
| GeoPandas 1.2 `to_parquet` (WKB and geoarrow) | GeoParquet 1.1 `crs` = full PROJJSON, id EPSG:4326 | PROJJSON, id OGC:CRS84 | PROJJSON, id EPSG:3857 / 2263 |
| GeoPandas `to_arrow` | field `crs` = PROJJSON, no `crs_type` | PROJJSON, id OGC:CRS84 | PROJJSON |
| GDAL 3.13 Parquet (WKB and GEOARROW) | **`crs` key omitted** (= OGC:CRS84 by spec) | omitted | PROJJSON |
| GDAL 3.13 Arrow IPC | field tag only, no CRS; schema-level `geo` metadata (GDAL's own, v0.1.0) has a WKT2 string, `ID["EPSG",4326]` | WKT2, `ID["OGC","CRS84"]` | WKT2 |
| pyogrio 0.13 `read_arrow` (GDAL 3.12 ArrowStream) | field `crs` = PROJJSON, no `crs_type` | PROJJSON | PROJJSON |
| DuckDB 1.5.6 `COPY` to Parquet | GeoParquet 1.0 `crs` = PROJJSON | **omitted** | PROJJSON |
| DuckDB `.arrow()` | `crs_type: projjson`, PROJJSON | `projjson`, id OGC:CRS84 | `projjson` |
| geoarrow-pyarrow 0.3 `with_crs` | a string: `{"crs":"EPSG:4326"}`, no `crs_type`; a pyproj CRS: PROJJSON with `crs_type: projjson` | same | same |
| geoarrow-rs `geoparquet` 0.8 writer (default options) | from an `authority_code` / `srid` / unknown-string CRS: **`crs` dropped silently** (`DefaultCrsTransform` returns `None`); from PROJJSON: copied | dropped | dropped unless PROJJSON |
| geodatafusion today (`ST_Point(.., srid)`, `ST_SetSRID`-style) | `{"crs":"EPSG:4326","crs_type":"authority_code"}` | — | `EPSG:3857` authority code |
| `fixtures/geoparquet/nybb_wkb.parquet` via geodatafusion-geoparquet | field `crs` = full PROJJSON with `"crs_type":"projjson"`, id EPSG:2263 (GeoParquet 1.0.0 file) | | |

Readers:

- **geodatafusion-geoparquet** (via geoparquet 0.8's `infer_geoarrow_schema`): a PROJJSON `crs`
  passes through. A **missing `crs` gives no CRS**, which is what GDAL and DuckDB write for WGS 84
  and means OGC:CRS84 by spec. A string `crs` such as `"EPSG:4326"` is labelled
  `crs_type: projjson` although it's a string.
- **DuckDB** reads GeoPandas' files as `GEOMETRY('EPSG:4326')`, `GEOMETRY('OGC:CRS84')` and
  `GEOMETRY('EPSG:3857')`.

### Round trips of a `geoarrow.wkb` column with a given tag

| tag | GeoPandas `from_arrow` → `to_arrow`/`to_parquet` | DuckDB `from_arrow` → `.arrow()`/`COPY` | GDAL (Arrow IPC file) | GDAL ipc → Parquet |
|---|---|---|---|---|
| `{"crs":"EPSG:4326","crs_type":"authority_code"}` | EPSG:4326 → PROJJSON (intact) | `GEOMETRY('EPSG:4326')` → PROJJSON (intact) | `ID["EPSG",4326]` (intact) | `crs` omitted (CRS84; intact) |
| `{"crs":"OGC:CRS84","crs_type":"authority_code"}` | OGC:CRS84 → PROJJSON (intact; `to_epsg()` = None) | `GEOMETRY('OGC:CRS84')` → PROJJSON; COPY omits `crs` (intact) | `ID["EPSG",4326]` (intact) | omitted (intact) |
| full PROJJSON of EPSG:4326 | intact | intact | intact | intact |
| `{"crs":"4326","crs_type":"srid"}` | EPSG:4326 (intact) | **`GEOMETRY` without CRS (lost)** | **unknown (lost)** | `crs: null` (lost) |
| `{"crs":"EPSG:4326"}` (no `crs_type`) | intact | intact | intact | intact |
| abbreviated PROJJSON (`type`, `name`, `id` only) | **fails: pyproj `Invalid projection`** | `GEOMETRY('EPSG:4326')` | **unknown** | — |

Plain Parquet with only Arrow-schema tags, as DataFusion's `COPY` writes it, loses the geometry or
the CRS in all three tools regardless of the CRS form. GeoPandas raises `Missing geo metadata`,
DuckDB reads `BLOB`, and GDAL recognises the geometry column but reports the SRS as `(unknown)`.

GeoParquet with a string `crs` (`"EPSG:4326"`, `"OGC:CRS84"`, `"EPSG:3857"`) in the `geo`
metadata:

- GeoPandas accepts it.
- DuckDB **fails**: `Geoparquet column 'geometry' has invalid CRS`.
- GDAL reports the SRS as unknown.
- geodatafusion-geoparquet passes it through as `crs_type: projjson` with a string value.

### Verdict

H9 holds. No external producer writes `EPSG:4326` as an authority code; they write full
PROJJSON or omit the GeoParquet `crs` (OGC:CRS84). Under the rule:

- **Map:** `util::srid::crs_to_srid` must give 4326 for:
  - PROJJSON whose `id` is EPSG:4326, or OGC:CRS84 (GeoPandas, DuckDB, pyogrio and GDAL Parquet
    write the latter id for CRS84).
  - `OGC:CRS84` as an authority code.
  - A WKT2 string with `ID["EPSG",4326]` or `ID["OGC","CRS84"]`.
  - A `geoarrow.*` field whose GeoParquet source had **no `crs` key**. This needs a fix in the
    reader (geodatafusion-geoparquet / geoparquet's `infer_geoarrow_schema`), which today
    turns a missing `crs` into no CRS. That reads as SRID 0, not 4326.
  - Strings without `crs_type` (geoarrow-pyarrow's `with_crs("EPSG:4326")`) and string values
    mislabelled `projjson`.
- **Write:** full PROJJSON is the only form that round-trips through every tool tested,
  including geoarrow-rs's GeoParquet writer and GeoParquet readers. `{"crs":"EPSG:4326",
  "crs_type":"authority_code"}` (D9's recommendation) round-trips through GeoPandas, DuckDB
  and GDAL in an Arrow hand-off. But geoarrow-rs's GeoParquet writer drops it silently, and as
  a GeoParquet string DuckDB rejects it. An abbreviated PROJJSON (just `id`) is not enough:
  GeoPandas/pyproj and GDAL reject it. Writing PROJJSON therefore means vendoring full PROJJSON
  per SRID, or deriving it with PROJ (D12), or keeping `EPSG:n` internally and supplying a
  `CrsTransform` that expands it when GeoParquet is written. The rule as written picks
  PROJJSON. A narrower reading (Arrow hand-off only) makes `EPSG:4326` a tie with PROJJSON.

## What changes the plan

1. **D2:**
   - H2b and H2c both count against native outputs. In DataFusion 54, CASE, COALESCE,
     make_array and array_agg drop extension metadata. So native geometry outputs fail in those
     constructs even when every branch has the same type. WKB outputs survive only as untagged
     `Binary` and lose the CRS there.
   - Update G6 R5's "cost" paragraph accordingly.
   - Check DataFusion 55 before treating this as fixed upstream. I didn't test it: geodatafusion
     isn't on DataFusion 55.
2. **D9:**
   - Recommend writing PROJJSON (full), not `EPSG:4326`, or at least expanding to PROJJSON when
     writing GeoParquet.
   - `crs_to_srid` needs the mappings listed above, and the GeoParquet reader must map a missing
     `crs` to OGC:CRS84.
   - G6 §4.4's "4326 for `OGC:CRS84`" is necessary but not enough.
3. **D3:**
   - Plain types are a bug fix (H3b).
   - Add to the bug list:
     - `CAST(geometry_or_tagged AS VARCHAR)` and UNION with literals keep `geoarrow.*` tags (a
       DataFusion 54 gap). This produces Parquet that geoarrow-pyarrow, GeoPandas and DuckDB
       can't read.
     - `ST_AsText(<plain Utf8>)` returns its input unparsed.
4. **D14 (upstream):**
   - geoarrow-rs: the GC collapse, the `PointBuilder` mixed-dimension panic and the
     `MultiPolygonBuilder` EMPTY-member panic are all still in 0.9.0 and on `main`.
   - geoarrow-rs: `DefaultCrsTransform` silently drops non-PROJJSON CRSs on GeoParquet write.
   - geoparquet: maps a missing `crs` to no CRS.
   - `wkt` 0.14: the writer panics on `MULTIPOLYGON(EMPTY, ...)`.
5. DataFusion's own Parquet output (`COPY`) is not GeoParquet, and no tool recovers the CRS from
   Arrow-schema tags alone. A GeoParquet writer is the only way to export geometries with their
   CRS.

## Threats to validity

- **DataFusion 54.1.0 instead of 54.0.0.** It's a patch release. The parity scan used the
  workspace build (54.0.0) and agrees with `parity.txt`.
- **"All WKB" is simulated** with `ST_AsBinary`, which tags the output `geoarrow.wkb` with the
  input CRS. A real WKB-returning function would carry the same type. The simulation can't show
  per-function costs (H2's job).
- **The H2b matrix uses one representative per type class.** Polygon/LineString native outputs
  behave like Point (a different `DataType` from `Union`/`Binary`). PI has no constant
  constructor, so its VALUES cell is n/a.
- **Parity scan:** only the first failure per record is visible. "Hidden" records were found by
  reading SQL, not by running them.
- **H2c counts rely on literal extraction and normalization.** Implicit-Z normalization assumes
  one dimension per literal. Records whose *intermediate* results are one-member collections
  (not visible in the SQL or expected output) aren't counted, so 9 is a lower bound.
- **H3b breakages come from geoarrow-pyarrow being imported.** Without it, pyarrow treats the tag
  as metadata and nothing breaks. GeoPandas sessions normally import it.
- **H9 DuckDB:** running the DuckDB round trip in the same process as pyogrio (bundled GDAL)
  segfaulted. The DuckDB steps run in subprocesses; in isolation they don't crash. I attribute it
  to a library clash, not to the CRS forms.
- **H9 covers one version per tool** (GeoPandas 1.2, GDAL 3.12/3.13, DuckDB 1.5.6). Older DuckDB
  (< 1.4) had no CRS-typed GEOMETRY.

## Reproduction

```bash
cd experiments/e4-type-model
./run_all.sh   # builds both crates, runs every experiment, rewrites the result files
```

Individual pieces (from `experiments/e4-type-model`, with
`CARGO_TARGET_DIR=/home/mikkel/Projects/geodatafusion/target/experiments/e4 RUSTUP_TOOLCHAIN=1.97.1`):

| Result file | Command |
|---|---|
| `h2b_results.md` | `(cd rust && cargo build --release --bins) && $CARGO_TARGET_DIR/release/h2b` |
| `slt_verbose.txt` | `cargo slt -v` from the repository root |
| `h2c_literals.json` | `python3 h2c_extract.py` |
| `h2c_repro_0.8.md` | `$CARGO_TARGET_DIR/release/h2c repro` |
| `h2c_roundtrip_0.8.jsonl` | `$CARGO_TARGET_DIR/release/h2c h2c_literals.json` |
| `h2c_repro_0.9.md` | `(cd rust-geoarrow09 && cargo build --release) && $CARGO_TARGET_DIR/release/e4-geoarrow09 h2c_literals.json` |
| `h3b_results.md` | `$CARGO_TARGET_DIR/release/h3b out/h3b` |
| `h3b_export_results.md` | `uv run --no-project --with geodatafusion --with datafusion --with pyarrow --with geoarrow-pyarrow --with geopandas --with duckdb python h3b_export.py out/h3b` |
| `h9_results.md` | `uv run --no-project --with pyarrow --with geoarrow-pyarrow --with geopandas --with pyogrio --with duckdb --with pyproj python h9_producers.py out/h9` (needs system `ogr2ogr`) |
| `h9_geo_string_crs.md` | `uv run ... python h9_geo_string_crs.py out/h9` |
| `h9_abbrev_projjson.md` | `uv run ... python h9_abbrev_projjson.py out/h9` |
| `h9_rust_results.md` | `$CARGO_TARGET_DIR/release/h9 out/h9_rust out/h9` (after the Python H9 scripts) |
| `postgis_reference.txt` | `psql postgresql://postgres:postgres@localhost:54329/postgres -Atf postgis_reference.sql` |
