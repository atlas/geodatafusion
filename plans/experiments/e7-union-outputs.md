# E7: native union outputs

Decision: D2 (output encoding), revisited for a single union output type.
Hypotheses (pre-registered in `plans/hypotheses.md`, unchanged): H2d, H2e, H2f, and the overall
D2 rule.

Run on 2026-10-06. Experiment code: `experiments/e7-union-outputs/` (four standalone crates plus
scripts). Raw results: `experiments/e7-union-outputs/results/`. Build output:
`target/experiments/e7`.

## Summary

| Hypothesis | Result | Verdict under the rule |
|---|---|---|
| H2d: union-only outputs make multi-step pipelines faster than WKB | WKB/union end to end, over 5 pipelines × 8 inputs: 0.68–1.00 in instructions and 0.69–1.03 in wall clock (best variants: 0.63–1.00 and 0.65–1.02). Union outputs are never ≥ 20% faster, on any input. With point outputs (p1, p2, p5 on points) they take 1.27–1.45× as long as WKB, and up to 1.22× on 10–100-vertex polygons. | **Refuted** (0 of 5 pipelines). |
| H2e: DataFusion 55 keeps extension metadata through CASE, COALESCE, make_array, array_agg, UNION ALL, VALUES and casts | DataFusion 55.1.0 keeps it through UNION ALL and VALUES only, the same as 54. CASE, COALESCE, make_array and array_agg drop it, for both `geoarrow.geometry` and `geoarrow.wkb`. Every cast tested drops it on 55, even `CAST(w AS BYTEA)`, which kept it on 54. | **Refuted.** CASE, COALESCE, make_array, array_agg and CAST drop the metadata, so native outputs stay blocked on DataFusion 55. |
| H2f: the one-member GEOMETRYCOLLECTION collapse can be avoided without forking geoarrow-rs | A local builder (149 code lines) or an 8-line upstream patch (1 insertion, 7 deletions) fixes the top-level collapse: 6 of the 9 records round-trip exactly, with no regressions in the other 1,149 literals. The other 3 are *nested* collections, which the GeoArrow spec forbids in `geoarrow.geometry`, so no builder can round-trip them. | **Refuted** (6 of 9, not all 9). The bug stays a blocker. For the nested cases the cause is the format, not geoarrow-rs. |
| Overall D2 rule: switch to union outputs only if H2d, H2e and H2f all hold | None holds. | **WKB outputs stay.** |

**What should change in the plan.**

- D2's revisit condition ("if DataFusion keeps extension metadata through those constructs and
  geoarrow-rs fixes the bug") can't be met for nested collections, whatever geoarrow-rs does.
  The GeoArrow spec defines the GeometryCollection child of `geoarrow.geometry` as a union of
  the six non-collection types, "to explicitly deny support for recursive geometry
  collections". PostGIS allows nested collections, and 3 doc-test inputs use them. Union
  outputs would also be slower (H2d). The condition should say that union outputs are out for
  parity, not deferred.
- geoarrow-rs 0.9.0 (released 2026-09-11) is already on arrow 59. geoarrow-array/-schema 0.9.0
  build and run with DataFusion 55.1.0 here. The phasing step "DataFusion 55 (needs a geoarrow
  release on arrow 59)" is not blocked on a release. I didn't build geodatafusion itself
  against 0.9.
- On DataFusion 55, `CAST(x AS BYTEA)` drops `geoarrow.wkb` and its CRS, which 54 kept. This
  also fixes E4's leak (`CAST(ST_AsBinary(g) AS VARCHAR)` no longer carries `geoarrow.wkb`).
  G6 batch 5 (DataFusion 55, remove the `::geometry` shim) should check whether anything relies
  on casts keeping metadata.
- Upstream: the one-member collapse fix is 8 lines
  (`experiments/e7-union-outputs/geoarrow-rs-no-gc-collapse.patch`), and geoarrow-array's tests
  pass with it (123 unit tests + 27 doc tests). It's worth filing with the other geoarrow-rs
  issues. Separately, `COALESCE(<Binary>, NULL)` fails to plan on DataFusion 54 and 55, with or
  without metadata ("Expect to get struct but got Binary"). That is a DataFusion bug that hits
  WKB outputs (`COALESCE(ST_Buffer(g, 1), NULL)`).

## Method

### H2d: speed

The E1 harness (`experiments/e1-performance`), copied and trimmed into
`experiments/e7-union-outputs/bench`, with only the typed loop style (the D1 decision):
`from_arrow_array` + `downcast_geoarrow_array!` into a generic function over
`GeoArrowArrayAccessor`, for producers and consumers. `data.rs`, `kernels.rs` and `cg.rs` are
E1's files, unchanged. The kernels (`t_x`, `t_area`, `t_centroid`, `t_simplify`, `t_translate`,
`t_buffer`, `t_astext`, prepared `t_intersects`) are E1's typed kernels. Only the output
builder differs.

Output variants for every geometry-producing function (ST_Centroid, ST_Simplify, ST_Translate,
ST_Buffer). The output is always `geoarrow.geometry` or `geoarrow.wkb`. ST_Centroid also returns
the union, not E1's `geoarrow.point`, because the hypothesis is "union-only".

| id | output | how |
|---|---|---|
| `n` | `geoarrow.geometry`, `CoordType::Separated` | geoarrow-array's `GeometryBuilder::push_geometry` |
| `l` | `geoarrow.geometry`, `CoordType::Separated` | the local builder from H2f (`shared/union_builder.rs`): geoarrow-array's public child builders, own type ids and offsets, `GeometryArray::new` |
| `w` | `geoarrow.wkb` | geoarrow-array's `WkbBuilder::push_geometry`, which calls `wkb::writer::write_geometry` (little endian) directly into the `BinaryBuilder` |
| `f` | `geoarrow.wkb` | a hand-written little-endian writer for `geo` Point/LineString/Polygon/MultiPolygon into the `BinaryBuilder`, falling back to `wkb::writer` |

- **Which WKB path is the real one.** `w` is what a real `map_geometry_to_wkb` would use: it is
  the `wkb` crate's writer with no intermediate copy. E1's fast writer (`fast_to_wkb`) converted
  native *inputs* to WKB, which D1 no longer does. Its output analogue is `f`, measured as a
  sensitivity variant. The verdict uses `n` vs `w` (the stock builders). I also report the best
  union variant against the best WKB variant ("best/best"). It doesn't change any verdict.
- **GEOS outputs (p4).** GEOS `WKBWriter` bytes are appended as they are for both WKB variants
  (`f` and `w` are identical there), as in E1 and G3. For union outputs the bytes are read with
  `wkb::reader::read_wkb` and pushed to the union builder (G3's `GeosGeometryBuilder`).
- **Correctness.** `verify` runs every producer on every input (3,000 rows) in all four
  variants. `n` and `l` give the same geometries as `w`, compared as WKT row by row, and `f` writes
  byte-identical WKB to `w`. All 32 checks pass (`results/verify.tsv`). Every query records a
  checksum of its result, and all four variants agree on every input, in cachegrind and both
  wall-clock passes (`analyze.py` checks: 0 mismatches).

Pipelines (consumers are typed and read whatever the producer returns through the same
downcast):

| id | SQL |
|---|---|
| p1 | `x(centroid_<o>(geom))` |
| p2 | `area(simplify_<o>(geom, 0.1))` |
| p3 | `astext(translate_<o>(geom, 1.0, 2.0))` |
| p4 | `intersects(buffer_<o>(geom, 0.1), e1_wkb('<100-vertex polygon>'))` |
| p5 | `x(centroid_<o>(simplify_<o>(translate_<o>(geom, 1.0, 2.0), 0.1)))` (the long chain) |

Inputs, row counts and measurement are E1's:

- `points`, `poly10`, `poly100`, `poly1000` (deterministic star polygons), each as native
  separated (`sep`) and WKB (`wkb`) input. Interleaved input was dropped; in E1 it matched
  separated.
- Wall clock: 1M points / 100k polygons (10k for `poly1000`) for p1–p3 and p5, and E1's
  ST_Buffer sizes for p4 (100k / 10k / 2k / 200). Cachegrind used 10× fewer rows (E1's
  `CG_ROWS`).
- `MemTable`, one partition of 8,192-row batches, `target_partitions = 1`, current-thread
  Tokio runtime.
- End-to-end time is `ctx.sql(q).await?.collect().await?`.
- Wall clock: one process per (pipeline, input), with the four variants interleaved in a
  rotating order, 1 warm-up and 7 measured repetitions. The ratio is the median of the
  per-repetition ratios. The whole matrix ran twice (`wall1`, `wall2`).
- Instructions: `valgrind --tool=cachegrind --cache-sim=no --instr-at-start=no`, counting only
  the end-to-end query after one uncounted warm-up, one run per configuration (E1 showed counts
  repeat to 0.1%).

Ratio: WKB / union, so > 1 means union outputs are faster. "≥ 20% faster" is read as ≥ 1.20 in
wall clock (≥ 1.25 if read as a time reduction), as in E1.

### H2e: SQL constructs on DataFusion 54 and 55

geoarrow-rs `main` (`02985efd`, 2026-09-11) and the 0.9.0 release both depend on arrow 59, the
version DataFusion 55 uses. So the test uses real geoarrow arrays on both versions:

- `sql/df55`: DataFusion 55.1.0, arrow 59.3.0, geoarrow-array/-schema 0.9.0.
- `sql/df54`: DataFusion 54.0.0, arrow 58.3.0, geoarrow-array/-schema 0.8.0 (control).

Both compile the same `sql/matrix.rs`:

- A 2-row `MemTable` with `id`, `g` (`geoarrow.geometry`, separated, CRS `EPSG:4326`), `w`
  (`geoarrow.wkb`, same CRS) and `b` (the same WKB as plain `Binary`, no metadata: control).
- `mk_union(id)` and `mk_wkb(id)`: UDFs whose `return_field_from_args` returns the
  extension-typed field (point `(id, id)`, CRS `EPSG:4326`). They stand in for a geometry
  function's output.
- Each construct runs with the column and with the UDF output as its source. VALUES needs
  constant rows, so it uses `mk_*(1)`, `mk_*(2)`, which constant-fold to literals.
- Two things are recorded:
  - `out`: the extension name and metadata on the planned output field
    (`df.schema().inner().field(0)`). For `make_array`/`array_agg`, the list's element field.
  - `udf`: what a UDF applied on top sees in `args.arg_fields` (`meta_of(x)` over the construct
    as a subquery), which is what geodatafusion's functions see.
- "Keeps" means both match the source exactly (name and CRS).

### H2f: the collapse

- `collapse/roundtrip.rs` reads E4's 1,231 extracted doc-test literals
  (`experiments/e4-type-model/h2c_literals.json`) and parses each with `wkt`; 1,158 parse.
- Each parsed literal goes builder → `GeometryArray` → Arrow `UnionArray` (`into_array_ref`,
  which runs arrow-rs's `UnionArray::try_new` validation) → `from_arrow_array` → `value(0)` →
  WKT. That is compared with the literal's own WKT, as in E4.
- Builders:
  - geoarrow-array 0.8.0's `GeometryBuilder` (baseline);
  - the local builder `shared/union_builder.rs`;
  - geoarrow-rs `main` with the patch `geoarrow-rs-no-gc-collapse.patch` (`collapse/patched`).
    I also ran geoarrow-array's own test suite with the patch.
- The local builder is the "different builder path" option. All of geoarrow-array's child
  builders and `GeometryArray::new` are public; only `GeometryBuilder::push_geometry_collection`
  is private, and `GeometryCollectionBuilder::push_geometry_collection` is public. So the local
  builder keeps one child builder per type and dimension, writes the dense-union type ids and
  offsets itself, and always routes a GEOMETRYCOLLECTION to the GeometryCollection child. No
  fork is needed.

## Environment

- AMD Ryzen 7 PRO 250 (8 cores / 16 threads), 60 GB RAM, Arch Linux, kernel 7.2.8. No other
  agents ran. Load average was 13–16 at the start of `wall1`, while the parallel cachegrind
  batch was winding down; that dropped to about 5 within a minute, and 5–6 during `wall2`
  (desktop, Firefox, a local Kubernetes).
- rustc 1.97.1 (`RUSTUP_TOOLCHAIN=1.97.1`), valgrind 3.25.1.
- H2d: DataFusion 54.0.0, arrow 58.3.0, geoarrow-array/-schema 0.8.0, geo 0.31.0, wkb 0.9.1,
  wkt 0.14.0, geos 11.1.1 (geos-sys 2.0.9) on system GEOS 3.15.0. Lock file copied from E1.
  Release profile as in E1 (`opt-level = 3`, `codegen-units = 16`, no LTO).
- H2e: as above for DataFusion 54. DataFusion 55.1.0 (the latest release; `main`'s
  `coalesce.rs` is unchanged) with arrow 59.3.0 and geoarrow-array/-schema 0.9.0.
- H2f: geoarrow-array 0.8.0, and geoarrow-rs `main` at `02985efd` (version 0.9.0) with the
  patch. wkt 0.14.0.

## H2d: speed

### Rule

Union outputs are faster if they're ≥ 20% faster end to end on at least two of the five
pipelines (H2's rule).

### Results

WKB / union, end to end, range over the 8 inputs (> 1: union faster). `w/n` is the stock
builders; `best/best` is min(`w`, `f`) / min(`n`, `l`).

| pipeline | instr w/n | instr best/best | wall1 w/n | wall1 best/best | wall2 w/n | wall2 best/best |
|---|---|---|---|---|---|---|
| p1 | 0.71–0.95 | 0.68–0.95 | 0.69–1.03 | 0.65–1.00 | 0.72–0.99 | 0.67–1.02 |
| p2 | 0.74–0.99 | 0.71–0.99 | 0.74–1.00 | 0.72–0.99 | 0.75–0.99 | 0.72–1.00 |
| p3 | 0.91–0.98 | 0.89–0.97 | 0.91–0.97 | 0.91–0.97 | 0.91–0.99 | 0.90–0.98 |
| p4 | 0.94–1.00 | 0.94–1.00 | 0.90–1.00 | 0.89–1.00 | 0.90–1.00 | 0.90–1.00 |
| p5 | 0.68–0.96 | 0.63–0.94 | 0.69–0.94 | 0.65–0.93 | 0.70–0.94 | 0.65–0.92 |

- No pipeline reaches 1.20 on any input, under either reading of "20%" or either pairing.
  The largest wall-clock ratio is 1.03 (`p1` on `poly1000`, within noise: per-repetition
  ratios 0.93–1.05).
- Point outputs are where union costs most: p1, p2 and p5 on points are 0.69–0.79 in wall
  clock, so the union pipelines take 1.27–1.45× as long. The long chain (p5) is the worst, because every step writes and
  reads a union.
- With polygons the fixed per-row costs are diluted, but union pipelines still take
  1.02–1.22× as long through 100 vertices. With 1,000 vertices the two are equal. Where GEOS dominates (p4) the ratio is 0.90–1.00, lowest for points.
- The local union builder (`l`) is within −1% to +5% of `GeometryBuilder` (`n`) in instructions. The
  hand-written WKB writer (`f`) is 0–11% faster than `w`. Neither changes any ratio enough to matter.
- Input encoding doesn't change the picture: `sep` and `wkb` ratios are within a few hundredths
  everywhere.
- The two wall-clock passes agree with each other and with the instruction counts.

Where the union cost goes, from `cg_annotate` for p1 on 100,000 points (`sep`): 2,180
instructions per row with union outputs, 1,553 with WKB.

- Union path:
  - `GeometryBuilder::push_point` and its per-push `flush_deferred_nulls`: 242 per row.
  - Reading back through `GeometryArray::value_unchecked` / `get_unchecked`: 212 per row.
  - Building and validating a 28-child `UnionArray` per batch (`UnionArray::try_new`, union
    field hashing and comparison): about 140 per row at 8,192 rows per batch.
- WKB path:
  - `wkb::reader` parsing (`Wkb::try_new`, `Point::try_new`, `WkbType::from_buffer`): 394 per
    row.
  - `wkb::writer::write_point`: 160 per row.

So the union's reads are not cheaper than WKB parsing, and its builder and per-batch array
assembly cost more than writing WKB.

Output sizes (buffer bytes, `results/h2d_sizes.tsv`, separated input). Union is 16% smaller for
point values (21 vs 25 bytes per row, including every ST_Centroid output) and within 0.1–3% for
polygons:

| producer | points (100k) union / WKB | poly10 (10k) | poly100 (2k) | poly1000 (200) |
|---|--:|--:|--:|--:|
| ST_Centroid | 2.10 / 2.50 MB | 0.21 / 0.25 MB | 42.3 / 50.0 kB | 4.5 / 5.0 kB |
| ST_Simplify | 2.10 / 2.50 MB | 1.498 / 1.537 MB | 1.392 / 1.400 MB | 0.397 / 0.398 MB |
| ST_Translate | 2.10 / 2.50 MB | 1.891 / 1.930 MB | 3.258 / 3.266 MB | 3.206 / 3.207 MB |
| ST_Buffer | 54.1 / 54.5 MB | 7.22 / 7.26 MB | 11.46 / 11.46 MB | 1.519 / 1.520 MB |

### Verdict

**Refuted.** Union-only outputs are ≥ 20% faster on 0 of the 5 pipelines, on any input, in
instructions and in both wall-clock passes. WKB outputs are as fast or faster everywhere. Union
outputs take 1.27–1.45× as long for point outputs and up to 1.22× for small polygons.

Comment on the rule: as in E1, "≥ 20% faster" doesn't say on how many inputs. That doesn't
matter here (none on any input). E1's one native win (p1 on points, 1.21–1.33) came from
`geoarrow.point` (a plain struct) output. A union-only output loses that case too.

## H2e: SQL constructs on DataFusion 55

### Rule

If every construct keeps the metadata, E4's objection to native outputs is removed on
DataFusion 55. Any construct that drops it keeps native outputs blocked there.

### Results

Condensed: the column and UDF sources gave identical results in every row (full matrix under
"Raw data"). "keeps" means the planned output field and a UDF on top both see the original
extension name and CRS. "drops" means both see no extension metadata.

| construct | union, DataFusion 54 | union, DataFusion 55 | WKB, DataFusion 54 | WKB, DataFusion 55 |
|---|---|---|---|---|
| projection (control) | keeps | keeps | keeps | keeps |
| `CASE WHEN .. THEN x ELSE x END` | drops | drops | drops | drops |
| `CASE WHEN .. THEN x END` | drops | drops | drops | drops |
| `COALESCE(x, x)` | drops | drops | drops | drops |
| `COALESCE(x, NULL)` | drops | drops | planning error¹ | planning error¹ |
| `unnest(make_array(x, x))` and the list's element field | drops | drops | drops | drops |
| `unnest(array_agg(x))` and the list's element field | drops | drops | drops | drops |
| `UNION ALL` | keeps | keeps | keeps | keeps |
| `VALUES (mk(1)), (mk(2))` | keeps | keeps | keeps | keeps |
| `CAST(x AS BYTEA)` (own storage type) | not expressible² | not expressible² | keeps | **drops** |
| `arrow_cast(x, '<own type>')` | not expressible² | not expressible² | plan drops, UDF sees it³ | drops |
| `CAST(x AS VARCHAR)` (should drop) | plan keeps the tag (wrong); execution error | drops (correct); execution error | plan keeps the tag (wrong); execution error | drops (correct); execution error |

1. `COALESCE(<Binary>, NULL)` fails to plan with `Function 'coalesce' user-defined coercion
   failed ... Expect to get struct but got Binary` on both versions. It fails the same way for
   the plain `Binary` control column, so it's a DataFusion coercion bug, not metadata.
2. SQL has no syntax for a dense union type. `arrow_cast` can't parse the union's `Display`
   string (`ParserError("Expected: ), found: vertices")`). A union-typed column can't be cast to
   its own type in SQL at all.
3. DataFusion 54 `arrow_cast(w, 'Binary')`: the planned field has no metadata, but the field a
   UDF on top receives still carries it, and the executed batch's metadata differs from the plan.

Where DataFusion 55 drops it, the cause is plain: `coalesce`'s `return_field_from_args` builds
`Field::new(name, type, nullable)` from the first argument's type (unchanged on `main`), and
COALESCE is simplified to CASE. DataFusion 55's new extension type registry (`DFExtensionType`)
only customises pretty-printing ("Currently, the following operations can be customized:
Pretty-printing values"), and nothing in coercion or CASE consults it.

### Verdict

**Refuted.** On DataFusion 55, CASE, COALESCE, make_array and array_agg still drop the extension
metadata of both `geoarrow.geometry` and `geoarrow.wkb`, exactly as on 54. Every cast tested now
drops it. Only UNION ALL and VALUES keep it. Under the rule, native outputs stay blocked on
DataFusion 55.

The same drops cost WKB outputs their CRS (E4's caveat), but a WKB array without metadata is
still `Binary`, which geodatafusion reads as WKB. A union array without metadata isn't readable
as a geometry by geoarrow-rs (E4: "Only FixedSizeList, Struct, Binary, ... are unambiguously
typed").

## H2f: the collapse

### Rule

If a workaround of ≤ 200 lines round-trips all 9 affected doc-test geometries exactly, the bug
isn't a blocker. Otherwise it is, until geoarrow-rs releases a fix.

### Results

The two workarounds:

- **Local builder:** `shared/union_builder.rs`, 166 lines (149 excluding blank and comment
  lines). It uses only geoarrow-array's public API, so no fork is needed.
- **Upstream patch:** `geoarrow-rs-no-gc-collapse.patch` against geoarrow-rs `main`, 8 changed
  lines (1 insertion, 7 deletions) in `builder/geometry.rs`. It replaces the
  `if gc.num_geometries() == 1` branch with `push_geometry_collection`. With the patch,
  geoarrow-array's tests pass (123 unit tests, 27 doc tests, `--all-features`).

Round trip of the 9 records E4 found:

| record | literal | geoarrow-array 0.8.0 `GeometryBuilder` | local builder | patched `main` |
|---|---|---|---|---|
| st_clusterintersecting.slt:8 (expected) | `GEOMETRYCOLLECTION(LINESTRING(6 6,7 7))` | `LINESTRING(6 6,7 7)` | exact | exact |
| st_clusterwithin.slt:8 (expected) | `GEOMETRYCOLLECTION(LINESTRING(6 6,7 7))` | `LINESTRING(6 6,7 7)` | exact | exact |
| st_collectionhomogenize.slt:8 (input) | `GEOMETRYCOLLECTION(POINT(0 0))` | `POINT(0 0)` | exact | exact |
| st_collectionhomogenize.slt:14 (input) | `GEOMETRYCOLLECTION(MULTIPOINT((0 0)))` | `MULTIPOINT((0 0))` | exact | exact |
| st_force_collection.slt:8 (expected) | `GEOMETRYCOLLECTION Z(POLYGON Z((0 0 1,0 5 1,5 0 1,0 0 1),(1 1 1,3 1 1,1 3 1,1 1 1)))` | `POLYGON Z(...)` | exact | exact |
| st_split.slt:25 (expected) | `GEOMETRYCOLLECTION(LINESTRING(0 0,100 100))` | `LINESTRING(0 0,100 100)` | exact | exact |
| st_collectionextract.slt:15 (input) | `GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(0 0)))` | `POINT(0 0)` | `GEOMETRYCOLLECTION(POINT(0 0))` | `GEOMETRYCOLLECTION(POINT(0 0))` |
| st_collectionextract.slt:23 (input) | `GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(LINESTRING(0 0, 1 1)),LINESTRING(2 2, 3 3))` | `GEOMETRYCOLLECTION(LINESTRING(0 0,1 1),LINESTRING(2 2,3 3))` | same as 0.8.0 | same as 0.8.0 |
| st_collectionhomogenize.slt:26 (input) | `GEOMETRYCOLLECTION(POINT(0 0), GEOMETRYCOLLECTION( LINESTRING(1 1, 2 2)))` | `GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(1 1,2 2))` | same as 0.8.0 | same as 0.8.0 |

Over all 1,158 parsed literals:

| builder | same | changed |
|---|--:|--:|
| geoarrow-array 0.8.0 `GeometryBuilder` | 1,149 | 9 |
| local builder (on 0.8.0) | 1,155 | 3 (the nested ones) |
| patched `main` `GeometryBuilder` | 1,155 | 3 (the nested ones) |
| local builder (on patched `main`) | 1,155 | 3 (the nested ones) |

- No regressions: every literal that round-tripped before still does.
- The three remaining failures are nested GEOMETRYCOLLECTIONs. Inside a collection,
  geoarrow-rs's mixed builder flattens a one-member nested collection into its member
  (`builder/mixed.rs:350`) and rejects a nested collection with more members ("nested geometry
  collections not supported in GeoArrow"). That isn't a geoarrow-rs shortcoming that a patch
  could fix. The GeoArrow format defines the GeometryCollection child as `List<DenseUnion>` over
  Point … MultiPolygon only, "in order to explicitly deny support for recursive geometry
  collections" (geoarrow `format.md`). In `geoarrow.wkb` they round-trip exactly (E4: 0 of 9
  changed).

### Verdict

**Refuted.** Both workarounds are well under 200 lines (149 and 8) and need no fork, but they
round-trip 6 of the 9 geometries exactly, not all 9. Under the rule the bug stays a blocker.

Comment on the rule: it assumes all 9 failures are one builder bug. They aren't. Six are the
fixable one-member collapse. Three are nested collections, which `geoarrow.geometry` can't
represent by specification, so the rule's escape clause ("until geoarrow-rs releases a fix")
can never apply to them. For union outputs this is a permanent parity gap, not a pending fix.

## Overall D2 rule

Switch to union outputs only if H2d, H2e and H2f all hold. All three are refuted, so **WKB
outputs stay**. Each of them alone would keep WKB.

## Threats to validity

- **Wall-clock noise.** The machine was not idle (load average about 5 during the runs, and
  13–16 for the first minute of `wall1` while cachegrind finished). Queries run on one core with
  interleaved repetitions. `wall1` and `wall2` agree within a few hundredths, and instruction
  counts give the same verdict. Spreads are in the raw tables.
- **One union builder strategy.** I measured geoarrow-array's `GeometryBuilder` and a thin local
  builder over the same child builders. A union builder written from scratch (preallocated
  child buffers, a single child type known in advance, no per-batch 28-child assembly) could be
  faster. But union *reads* (212 instructions per row for points) alone cost about as much as
  the whole WKB write, and the gap to 1.20 in the other direction is 30–50%. The verdict is
  unlikely to flip.
- **GEOS → union through WKB.** p4's union variant parses GEOS's WKB before pushing it to the
  union builder. A direct GEOS→GeoArrow path would remove the parse, but p4 is within 0–10%
  anyway; GEOS dominates.
- **Synthetic inputs** (E1's): XY only, no holes, no multi-geometries, no NULLs or EMPTYs.
  Union outputs of mixed geometry types would add more children per batch, not fewer.
- **H2e coverage.** Two-row tables, one CRS, no mixed-type sources (E4 covered those). The
  matrix tests what was pre-registered, plus `CASE` without `ELSE` and `COALESCE(x, NULL)`. A
  construct that drops metadata in a 2-row table also drops it in a larger one, because the drop
  happens at planning.
- **H2f literals** are the 1,158 that the `wkt` crate parses (E4's normalization). Curves are
  out of scope, as in E4.

## Reproduce

```sh
cd experiments/e7-union-outputs
export RUSTUP_TOOLCHAIN=1.97.1
export CARGO_TARGET_DIR=$PWD/../../target/experiments/e7
B=$CARGO_TARGET_DIR/release

# H2d
(cd bench && cargo build --release)
for d in points poly10 poly100 poly1000; do for e in sep wkb; do
  $B/e7-bench verify $d $e 3000; done; done > results/verify.tsv
python3 bench/scripts/run_cg.py --jobs 8          # results/h2d_cg.tsv
python3 bench/scripts/run_wall.py --tag wall1     # results/h2d_wall1.tsv
python3 bench/scripts/run_wall.py --tag wall2
for d in points:100000 poly10:10000 poly100:2000 poly1000:200; do
  $B/e7-bench sizes ${d%%:*} sep ${d##*:}; done > results/h2d_sizes.tsv
python3 bench/scripts/analyze.py wall1 wall2 > results/h2d_tables.md
# one query by hand
$B/e7-bench run points sep 1000000 7 p1_n p1_l p1_w p1_f
valgrind --tool=cachegrind --cache-sim=no --instr-at-start=no $B/e7-bench cg points sep 100000 p1_n

# H2e
(cd sql/df54 && cargo build --release) && $B/e7-sql-df54 > results/h2e_df54.tsv
(cd sql/df55 && cargo build --release) && $B/e7-sql-df55 > results/h2e_df55.tsv
python3 sql/summarize.py > results/h2e_table.md

# H2f
git clone https://github.com/geoarrow/geoarrow-rs $CARGO_TARGET_DIR/src/geoarrow-rs
(cd $CARGO_TARGET_DIR/src/geoarrow-rs && git checkout 02985efd \
  && git apply $OLDPWD/geoarrow-rs-no-gc-collapse.patch)
(cd collapse/g08 && cargo build --release)
(cd collapse/patched && cargo build --release)
L=../e4-type-model/h2c_literals.json
$B/e7-collapse-g08 geoarrow-0.8.0 $L > results/h2f_g08.jsonl 2> results/h2f_g08_summary.tsv
$B/e7-collapse-patched geoarrow-main-patched $L > results/h2f_patched.jsonl 2> results/h2f_patched_summary.tsv
(cd $CARGO_TARGET_DIR/src/geoarrow-rs && cargo test -p geoarrow-array --all-features)
```

Query ids are `<p1..p5>_<n|l|w|f>`; `e7-bench list` prints the SQL.

## Raw data

#### H2e: full matrix (`results/h2e_df54.tsv`, `results/h2e_df55.tsv`)

"yes": the planned output field and a UDF on top both carry the source's extension name and
CRS. "NO": neither does. "udf only": only the UDF's argument field does. The TSV files also
contain the exact SQL and the metadata strings.

| kind | source | construct | DataFusion 54 | DataFusion 55 |
|---|---|---|---|---|
| union | column | control: projection | yes | yes |
| union | column | CASE (two branches) | NO | NO |
| union | column | CASE (ELSE NULL) | NO | NO |
| union | column | COALESCE(x, x) | NO | NO |
| union | column | COALESCE(x, NULL) | NO | NO |
| union | column | make_array + unnest | NO | NO |
| union | column | make_array (element field) | NO | NO |
| union | column | array_agg + unnest | NO | NO |
| union | column | array_agg (element field) | NO | NO |
| union | column | UNION ALL | yes | yes |
| union | column | CAST to own storage type | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 29") | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 29") |
| union | column | arrow_cast to own storage type | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 198") | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 198") |
| union | column | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| union | udf | control: projection | yes | yes |
| union | udf | CASE (two branches) | NO | NO |
| union | udf | CASE (ELSE NULL) | NO | NO |
| union | udf | COALESCE(x, x) | NO | NO |
| union | udf | COALESCE(x, NULL) | NO | NO |
| union | udf | make_array + unnest | NO | NO |
| union | udf | make_array (element field) | NO | NO |
| union | udf | array_agg + unnest | NO | NO |
| union | udf | array_agg (element field) | NO | NO |
| union | udf | UNION ALL | yes | yes |
| union | udf | VALUES (constants) | yes | yes |
| union | udf | CAST to own storage type | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 40") | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 40") |
| union | udf | arrow_cast to own storage type | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 209") | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 209") |
| union | udf | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| wkb | column | control: projection | yes | yes |
| wkb | column | CASE (two branches) | NO | NO |
| wkb | column | CASE (ELSE NULL) | NO | NO |
| wkb | column | COALESCE(x, x) | NO | NO |
| wkb | column | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| wkb | column | make_array + unnest | NO | NO |
| wkb | column | make_array (element field) | NO | NO |
| wkb | column | array_agg + unnest | NO | NO |
| wkb | column | array_agg (element field) | NO | NO |
| wkb | column | UNION ALL | yes | yes |
| wkb | column | CAST to own storage type | yes | NO |
| wkb | column | arrow_cast to own storage type | udf only | NO |
| wkb | column | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| wkb | udf | control: projection | yes | yes |
| wkb | udf | CASE (two branches) | NO | NO |
| wkb | udf | CASE (ELSE NULL) | NO | NO |
| wkb | udf | COALESCE(x, x) | NO | NO |
| wkb | udf | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| wkb | udf | make_array + unnest | NO | NO |
| wkb | udf | make_array (element field) | NO | NO |
| wkb | udf | array_agg + unnest | NO | NO |
| wkb | udf | array_agg (element field) | NO | NO |
| wkb | udf | UNION ALL | yes | yes |
| wkb | udf | VALUES (constants) | yes | yes |
| wkb | udf | CAST to own storage type | yes | NO |
| wkb | udf | arrow_cast to own storage type | udf only | NO |
| wkb | udf | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| plain | column | control: projection | n/a (control, works) | n/a (control, works) |
| plain | column | CASE (two branches) | n/a (control, works) | n/a (control, works) |
| plain | column | CASE (ELSE NULL) | n/a (control, works) | n/a (control, works) |
| plain | column | COALESCE(x, x) | n/a (control, works) | n/a (control, works) |
| plain | column | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| plain | column | make_array + unnest | n/a (control, works) | n/a (control, works) |
| plain | column | make_array (element field) | n/a (control, works) | n/a (control, works) |
| plain | column | array_agg + unnest | n/a (control, works) | n/a (control, works) |
| plain | column | array_agg (element field) | n/a (control, works) | n/a (control, works) |
| plain | column | UNION ALL | n/a (control, works) | n/a (control, works) |
| plain | column | CAST to own storage type | n/a (control, works) | n/a (control, works) |
| plain | column | arrow_cast to own storage type | n/a (control, works) | n/a (control, works) |
| plain | column | CAST to VARCHAR (should drop) | n/a (control, exec error); no tag in plan; exec error | n/a (control, exec error); no tag in plan; exec error |

#### H2d: Instructions per row (cachegrind, end to end)

| pipeline | dataset | enc | rows | n | l | w | f | w/n | best/best |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 100,000 | 2,180 | 2,068 | 1,553 | 1,398 | 0.71 | 0.68 |
| p1 | points | wkb | 100,000 | 2,693 | 2,577 | 2,068 | 1,914 | 0.77 | 0.74 |
| p1 | poly10 | sep | 10,000 | 6,848 | 6,732 | 5,140 | 4,978 | 0.75 | 0.74 |
| p1 | poly10 | wkb | 10,000 | 6,945 | 6,829 | 5,270 | 5,108 | 0.76 | 0.75 |
| p1 | poly100 | sep | 10,000 | 27,742 | 27,573 | 26,041 | 25,894 | 0.94 | 0.94 |
| p1 | poly100 | wkb | 10,000 | 23,579 | 23,561 | 22,029 | 21,880 | 0.93 | 0.93 |
| p1 | poly1000 | sep | 1,000 | 243,513 | 243,398 | 230,672 | 230,451 | 0.95 | 0.95 |
| p1 | poly1000 | wkb | 1,000 | 198,237 | 198,099 | 185,173 | 185,022 | 0.93 | 0.93 |
| p2 | points | sep | 100,000 | 2,557 | 2,440 | 1,880 | 1,725 | 0.74 | 0.71 |
| p2 | points | wkb | 100,000 | 3,086 | 2,966 | 2,410 | 2,255 | 0.78 | 0.76 |
| p2 | poly10 | sep | 10,000 | 26,254 | 26,470 | 23,824 | 23,357 | 0.91 | 0.89 |
| p2 | poly10 | wkb | 10,000 | 26,070 | 26,084 | 23,745 | 23,098 | 0.91 | 0.89 |
| p2 | poly100 | sep | 10,000 | 232,183 | 232,755 | 227,383 | 225,957 | 0.98 | 0.97 |
| p2 | poly100 | wkb | 10,000 | 228,863 | 228,192 | 223,891 | 221,966 | 0.98 | 0.97 |
| p2 | poly1000 | sep | 1,000 | 2,961,605 | 2,962,415 | 2,937,057 | 2,931,607 | 0.99 | 0.99 |
| p2 | poly1000 | wkb | 1,000 | 2,913,254 | 2,917,204 | 2,891,816 | 2,885,488 | 0.99 | 0.99 |
| p3 | points | sep | 100,000 | 6,565 | 6,445 | 5,942 | 5,757 | 0.91 | 0.89 |
| p3 | points | wkb | 100,000 | 7,080 | 6,962 | 6,472 | 6,284 | 0.91 | 0.90 |
| p3 | poly10 | sep | 10,000 | 56,481 | 56,415 | 53,982 | 53,191 | 0.96 | 0.94 |
| p3 | poly10 | wkb | 10,000 | 56,610 | 56,486 | 53,904 | 53,192 | 0.95 | 0.94 |
| p3 | poly100 | sep | 10,000 | 473,981 | 474,126 | 462,474 | 457,882 | 0.98 | 0.97 |
| p3 | poly100 | wkb | 10,000 | 470,340 | 470,223 | 458,419 | 453,871 | 0.97 | 0.97 |
| p3 | poly1000 | sep | 1,000 | 4,653,322 | 4,653,214 | 4,536,714 | 4,492,432 | 0.97 | 0.97 |
| p3 | poly1000 | wkb | 1,000 | 4,607,852 | 4,607,734 | 4,491,217 | 4,446,941 | 0.97 | 0.97 |
| p4 | points | sep | 10,000 | 242,816 | 242,933 | 231,692 | 231,692 | 0.95 | 0.95 |
| p4 | points | wkb | 10,000 | 243,779 | 243,290 | 232,159 | 232,159 | 0.95 | 0.95 |
| p4 | poly10 | sep | 1,000 | 380,892 | 381,845 | 356,917 | 356,916 | 0.94 | 0.94 |
| p4 | poly10 | wkb | 1,000 | 381,158 | 380,808 | 356,443 | 356,444 | 0.94 | 0.94 |
| p4 | poly100 | sep | 200 | 6,270,163 | 6,269,262 | 6,094,792 | 6,094,793 | 0.97 | 0.97 |
| p4 | poly100 | wkb | 200 | 6,266,160 | 6,263,014 | 6,093,267 | 6,093,268 | 0.97 | 0.97 |
| p4 | poly1000 | sep | 50 | 495,956,946 | 495,957,294 | 495,575,785 | 495,575,785 | 1.00 | 1.00 |
| p4 | poly1000 | wkb | 50 | 495,897,942 | 495,890,234 | 495,524,555 | 495,524,556 | 1.00 | 1.00 |
| p5 | points | sep | 100,000 | 6,508 | 6,156 | 4,401 | 3,902 | 0.68 | 0.63 |
| p5 | points | wkb | 100,000 | 7,027 | 6,682 | 4,910 | 4,437 | 0.70 | 0.66 |
| p5 | poly10 | sep | 10,000 | 39,066 | 38,486 | 31,474 | 30,405 | 0.81 | 0.79 |
| p5 | poly10 | wkb | 10,000 | 38,913 | 38,538 | 31,634 | 30,076 | 0.81 | 0.78 |
| p5 | poly100 | sep | 10,000 | 277,112 | 277,740 | 259,949 | 253,163 | 0.94 | 0.91 |
| p5 | poly100 | wkb | 10,000 | 273,991 | 273,600 | 258,205 | 249,299 | 0.94 | 0.91 |
| p5 | poly1000 | sep | 1,000 | 3,343,023 | 3,335,016 | 3,195,946 | 3,147,023 | 0.96 | 0.94 |
| p5 | poly1000 | wkb | 1,000 | 3,292,402 | 3,292,513 | 3,151,242 | 3,102,073 | 0.96 | 0.94 |

#### H2d: Wall clock (wall1): median ms (min–max), 7 repetitions

| pipeline | dataset | enc | rows | n | l | w | f | w/n median (min–max of per-rep ratios) |
|---|---|---|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 1,000,000 | 82.6 (79.4–84.3) | 81.9 (80.2–83.6) | 57.2 (55.7–60.1) | 53.4 (52.4–55.3) | 0.69 (0.66–0.75) |
| p1 | points | wkb | 1,000,000 | 99.8 (95.7–100.7) | 97.4 (95.8–101.0) | 73.0 (72.2–74.6) | 69.0 (68.3–73.1) | 0.73 (0.72–0.78) |
| p1 | poly10 | sep | 100,000 | 27.1 (26.6–29.1) | 27.0 (26.3–27.5) | 23.7 (23.4–23.8) | 23.4 (23.2–26.2) | 0.88 (0.81–0.88) |
| p1 | poly10 | wkb | 100,000 | 22.6 (22.1–24.9) | 22.0 (21.5–23.0) | 19.1 (18.8–20.0) | 18.5 (18.2–19.4) | 0.85 (0.77–0.87) |
| p1 | poly100 | sep | 100,000 | 142.7 (141.2–148.4) | 143.7 (140.8–149.3) | 139.3 (137.5–145.7) | 142.5 (136.9–145.0) | 0.97 (0.95–1.03) |
| p1 | poly100 | wkb | 100,000 | 85.5 (84.3–89.0) | 85.6 (83.7–87.3) | 82.1 (79.7–88.1) | 82.5 (79.1–84.5) | 0.96 (0.92–1.03) |
| p1 | poly1000 | sep | 10,000 | 129.9 (128.5–136.5) | 131.5 (128.8–139.8) | 133.9 (127.7–138.6) | 129.4 (127.8–134.3) | 1.03 (0.94–1.05) |
| p1 | poly1000 | wkb | 10,000 | 70.5 (70.2–75.6) | 70.1 (69.8–75.7) | 69.8 (69.0–74.2) | 69.8 (69.2–72.7) | 0.99 (0.93–1.05) |
| p2 | points | sep | 1,000,000 | 98.2 (96.3–101.3) | 97.3 (96.1–99.1) | 73.0 (72.2–75.2) | 70.0 (68.9–72.5) | 0.74 (0.72–0.78) |
| p2 | points | wkb | 1,000,000 | 115.6 (113.4–119.0) | 113.1 (112.2–116.3) | 91.0 (89.6–93.5) | 84.2 (83.5–87.9) | 0.79 (0.77–0.82) |
| p2 | poly10 | sep | 100,000 | 102.5 (99.3–104.0) | 101.3 (98.7–103.5) | 92.5 (91.5–96.1) | 90.4 (88.7–91.0) | 0.90 (0.88–0.97) |
| p2 | poly10 | wkb | 100,000 | 96.7 (95.6–101.6) | 95.5 (94.8–99.4) | 87.5 (86.4–89.5) | 85.2 (84.5–88.2) | 0.91 (0.85–0.92) |
| p2 | poly100 | sep | 100,000 | 938.9 (936.3–952.4) | 936.9 (933.6–947.1) | 918.4 (907.7–921.3) | 913.7 (900.0–929.1) | 0.98 (0.96–0.98) |
| p2 | poly100 | wkb | 100,000 | 886.6 (884.0–893.1) | 885.0 (879.9–888.5) | 865.3 (850.9–867.8) | 851.0 (842.5–857.3) | 0.98 (0.96–0.98) |
| p2 | poly1000 | sep | 10,000 | 1099.2 (1089.5–1104.6) | 1100.2 (1094.7–1109.9) | 1091.2 (1085.1–1095.3) | 1091.4 (1083.2–1098.4) | 0.99 (0.99–1.00) |
| p2 | poly1000 | wkb | 10,000 | 1033.0 (1021.7–1060.2) | 1030.2 (1023.8–1084.6) | 1026.6 (1013.9–1051.6) | 1015.8 (1009.6–1040.9) | 1.00 (0.96–1.02) |
| p3 | points | sep | 1,000,000 | 232.6 (229.5–238.1) | 230.8 (228.5–233.7) | 211.6 (210.3–215.6) | 210.7 (206.8–217.9) | 0.91 (0.89–0.94) |
| p3 | points | wkb | 1,000,000 | 262.2 (251.9–269.5) | 254.0 (248.1–280.1) | 243.3 (236.0–248.8) | 232.0 (229.4–238.3) | 0.94 (0.90–0.97) |
| p3 | poly10 | sep | 100,000 | 195.3 (192.3–199.1) | 188.1 (186.1–195.9) | 182.8 (181.6–186.9) | 183.3 (179.2–188.8) | 0.94 (0.91–0.95) |
| p3 | poly10 | wkb | 100,000 | 187.8 (184.8–193.1) | 182.3 (181.4–194.8) | 175.9 (174.8–181.0) | 175.4 (173.0–176.5) | 0.95 (0.91–0.96) |
| p3 | poly100 | sep | 100,000 | 1612.4 (1603.7–1715.1) | 1619.4 (1586.5–1796.1) | 1539.6 (1501.3–1553.2) | 1534.6 (1492.6–1698.4) | 0.96 (0.90–0.96) |
| p3 | poly100 | wkb | 100,000 | 1541.1 (1535.4–1550.5) | 1538.7 (1535.8–1554.1) | 1471.1 (1467.5–1477.7) | 1451.6 (1448.7–1470.9) | 0.96 (0.95–0.96) |
| p3 | poly1000 | sep | 10,000 | 1535.2 (1532.4–1545.5) | 1535.5 (1531.1–1541.0) | 1461.3 (1454.5–1465.1) | 1448.1 (1442.6–1456.3) | 0.95 (0.94–0.96) |
| p3 | poly1000 | wkb | 10,000 | 1421.5 (1413.9–1437.1) | 1418.3 (1414.5–1441.0) | 1376.6 (1360.8–1400.8) | 1358.3 (1345.1–1382.2) | 0.97 (0.95–0.99) |
| p4 | points | sep | 100,000 | 768.7 (753.0–790.2) | 763.3 (746.0–776.9) | 693.2 (681.4–703.1) | 687.6 (679.4–732.9) | 0.91 (0.89–0.91) |
| p4 | points | wkb | 100,000 | 769.5 (764.3–797.8) | 767.9 (751.5–783.4) | 686.7 (685.3–716.1) | 695.9 (682.2–707.3) | 0.90 (0.88–0.90) |
| p4 | poly10 | sep | 10,000 | 115.8 (115.1–120.1) | 116.1 (114.7–121.5) | 112.5 (108.8–116.5) | 110.8 (108.8–125.5) | 0.97 (0.91–1.01) |
| p4 | poly10 | wkb | 10,000 | 114.0 (113.0–118.1) | 116.1 (113.5–118.8) | 109.9 (107.0–112.5) | 107.9 (107.3–110.9) | 0.96 (0.91–0.98) |
| p4 | poly100 | sep | 2,000 | 536.2 (528.6–546.1) | 540.1 (528.7–545.5) | 534.7 (516.4–540.8) | 526.9 (515.6–538.4) | 0.98 (0.98–1.02) |
| p4 | poly100 | wkb | 2,000 | 541.7 (522.5–547.8) | 539.4 (522.3–544.9) | 518.9 (516.8–539.4) | 520.7 (514.3–537.5) | 0.99 (0.95–1.00) |
| p4 | poly1000 | sep | 200 | 5530.2 (5492.2–5772.0) | 5663.9 (5466.8–5790.2) | 5552.2 (5521.2–5703.9) | 5678.4 (5519.8–5833.0) | 1.00 (0.96–1.04) |
| p4 | poly1000 | wkb | 200 | 5550.5 (5417.9–5926.0) | 5606.7 (5357.6–5839.7) | 5394.7 (5334.8–5843.0) | 5529.5 (5377.7–5677.5) | 0.98 (0.90–1.05) |
| p5 | points | sep | 1,000,000 | 234.2 (233.2–236.2) | 226.2 (225.7–227.1) | 161.2 (160.7–161.4) | 148.5 (148.0–150.4) | 0.69 (0.68–0.69) |
| p5 | points | wkb | 1,000,000 | 260.7 (259.1–261.7) | 252.0 (251.5–253.1) | 180.8 (180.7–182.8) | 164.6 (164.0–166.4) | 0.69 (0.69–0.70) |
| p5 | poly10 | sep | 100,000 | 155.6 (152.5–159.3) | 150.9 (149.9–159.6) | 127.3 (125.2–130.8) | 122.6 (121.1–125.5) | 0.83 (0.79–0.84) |
| p5 | poly10 | wkb | 100,000 | 149.4 (148.4–153.7) | 148.4 (147.2–150.6) | 123.0 (121.8–124.3) | 118.0 (116.9–122.9) | 0.82 (0.80–0.83) |
| p5 | poly100 | sep | 100,000 | 1124.5 (1122.3–1151.4) | 1123.0 (1118.4–1163.6) | 1019.4 (1010.2–1030.9) | 985.1 (979.0–1004.5) | 0.91 (0.88–0.92) |
| p5 | poly100 | wkb | 100,000 | 1062.2 (1061.7–1066.3) | 1065.6 (1062.6–1071.3) | 965.6 (958.9–972.7) | 934.7 (925.8–951.2) | 0.91 (0.90–0.92) |
| p5 | poly1000 | sep | 10,000 | 1264.6 (1259.1–1291.3) | 1263.3 (1255.7–1301.0) | 1180.5 (1173.2–1197.3) | 1164.4 (1156.9–1184.0) | 0.93 (0.91–0.95) |
| p5 | poly1000 | wkb | 10,000 | 1211.7 (1192.9–1246.4) | 1222.4 (1196.6–1248.0) | 1149.7 (1111.6–1169.3) | 1127.9 (1100.9–1136.2) | 0.94 (0.93–0.96) |

#### H2d: Wall clock (wall2): median ms (min–max), 7 repetitions

| pipeline | dataset | enc | rows | n | l | w | f | w/n median (min–max of per-rep ratios) |
|---|---|---|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 1,000,000 | 80.8 (80.0–85.9) | 82.4 (80.7–90.2) | 57.6 (56.8–60.7) | 54.0 (53.7–55.4) | 0.72 (0.66–0.75) |
| p1 | points | wkb | 1,000,000 | 96.7 (95.7–97.5) | 96.6 (95.8–98.4) | 72.9 (72.2–73.7) | 68.9 (68.6–70.2) | 0.75 (0.75–0.76) |
| p1 | poly10 | sep | 100,000 | 26.6 (26.5–27.2) | 26.3 (26.2–26.8) | 23.5 (23.4–23.9) | 23.3 (23.1–23.4) | 0.88 (0.86–0.90) |
| p1 | poly10 | wkb | 100,000 | 22.2 (22.1–22.3) | 21.6 (21.5–22.0) | 19.0 (18.8–19.4) | 18.7 (18.5–19.1) | 0.86 (0.85–0.88) |
| p1 | poly100 | sep | 100,000 | 146.2 (141.9–149.9) | 142.4 (141.6–146.1) | 138.5 (137.4–142.4) | 138.3 (137.0–144.1) | 0.96 (0.92–0.98) |
| p1 | poly100 | wkb | 100,000 | 89.7 (84.9–91.4) | 87.6 (84.5–90.3) | 81.9 (81.2–86.4) | 86.4 (82.7–87.1) | 0.93 (0.89–0.97) |
| p1 | poly1000 | sep | 10,000 | 133.3 (128.5–142.3) | 129.0 (128.4–136.5) | 130.2 (128.5–136.4) | 130.3 (127.6–136.3) | 0.98 (0.94–1.03) |
| p1 | poly1000 | wkb | 10,000 | 73.0 (70.4–75.9) | 70.4 (70.1–72.2) | 72.1 (69.0–74.5) | 71.9 (69.3–73.5) | 0.99 (0.98–1.00) |
| p2 | points | sep | 1,000,000 | 98.2 (97.1–98.9) | 97.9 (96.8–98.0) | 73.4 (72.3–74.2) | 70.4 (69.5–71.3) | 0.75 (0.73–0.76) |
| p2 | points | wkb | 1,000,000 | 114.2 (113.6–115.8) | 113.0 (112.5–113.7) | 90.9 (90.0–91.8) | 84.8 (84.4–85.8) | 0.79 (0.78–0.80) |
| p2 | poly10 | sep | 100,000 | 99.7 (99.1–102.7) | 99.7 (98.1–101.4) | 91.9 (90.5–92.7) | 89.6 (87.5–90.3) | 0.91 (0.90–0.93) |
| p2 | poly10 | wkb | 100,000 | 96.4 (95.3–100.0) | 95.4 (95.0–98.6) | 86.8 (85.8–91.2) | 84.7 (83.5–87.1) | 0.90 (0.89–0.91) |
| p2 | poly100 | sep | 100,000 | 932.6 (925.4–949.8) | 947.1 (930.1–965.6) | 926.7 (901.6–938.3) | 912.1 (895.5–938.3) | 0.99 (0.97–1.01) |
| p2 | poly100 | wkb | 100,000 | 879.1 (870.7–911.3) | 877.6 (874.7–903.5) | 869.9 (844.8–889.3) | 841.1 (831.7–868.3) | 0.97 (0.94–1.02) |
| p2 | poly1000 | sep | 10,000 | 1082.1 (1078.9–1164.3) | 1080.8 (1077.8–1109.7) | 1076.7 (1069.7–1087.9) | 1076.2 (1064.9–1143.9) | 0.99 (0.92–1.00) |
| p2 | poly1000 | wkb | 10,000 | 1029.6 (1021.2–1061.3) | 1024.4 (1020.2–1038.2) | 1017.6 (1011.7–1026.7) | 1026.4 (1008.0–1041.5) | 0.99 (0.96–1.01) |
| p3 | points | sep | 1,000,000 | 230.3 (229.5–240.9) | 229.4 (228.5–235.4) | 210.5 (210.3–211.9) | 207.4 (207.3–213.1) | 0.91 (0.87–0.92) |
| p3 | points | wkb | 1,000,000 | 254.8 (250.5–264.4) | 254.3 (249.5–259.1) | 235.6 (230.5–241.1) | 229.7 (224.6–233.7) | 0.93 (0.90–0.96) |
| p3 | poly10 | sep | 100,000 | 182.7 (180.9–186.1) | 182.4 (180.0–186.0) | 181.0 (180.0–183.9) | 178.5 (177.5–179.1) | 0.99 (0.97–1.01) |
| p3 | poly10 | wkb | 100,000 | 184.2 (182.0–187.7) | 183.4 (181.5–187.7) | 174.6 (174.0–176.0) | 171.9 (171.5–172.5) | 0.95 (0.93–0.97) |
| p3 | poly100 | sep | 100,000 | 1567.9 (1556.2–1611.5) | 1577.3 (1564.1–1613.1) | 1533.0 (1489.8–1544.0) | 1519.6 (1474.2–1527.6) | 0.96 (0.95–0.98) |
| p3 | poly100 | wkb | 100,000 | 1537.6 (1509.6–1587.1) | 1527.7 (1507.8–1571.7) | 1459.5 (1444.2–1477.0) | 1444.3 (1430.3–1518.1) | 0.96 (0.92–0.96) |
| p3 | poly1000 | sep | 10,000 | 1528.9 (1518.3–1546.2) | 1530.7 (1521.6–1539.8) | 1461.0 (1454.5–1468.2) | 1449.2 (1445.3–1459.5) | 0.96 (0.95–0.96) |
| p3 | poly1000 | wkb | 10,000 | 1469.7 (1441.9–1484.7) | 1468.2 (1443.4–1477.7) | 1399.8 (1391.7–1409.5) | 1381.3 (1374.9–1391.9) | 0.95 (0.94–0.97) |
| p4 | points | sep | 100,000 | 778.8 (764.3–786.7) | 775.6 (764.8–789.9) | 702.7 (700.6–708.0) | 701.0 (698.5–706.2) | 0.90 (0.90–0.92) |
| p4 | points | wkb | 100,000 | 778.1 (773.5–787.2) | 785.7 (772.5–792.9) | 703.5 (699.6–710.2) | 703.8 (698.9–706.2) | 0.90 (0.90–0.91) |
| p4 | poly10 | sep | 10,000 | 120.1 (119.5–123.9) | 120.2 (119.7–121.7) | 113.8 (113.5–115.2) | 113.7 (113.2–114.8) | 0.95 (0.92–0.96) |
| p4 | poly10 | wkb | 10,000 | 117.5 (117.2–118.9) | 117.7 (116.9–118.5) | 111.9 (110.9–112.1) | 111.4 (111.1–112.0) | 0.95 (0.94–0.95) |
| p4 | poly100 | sep | 2,000 | 539.0 (537.9–540.6) | 539.8 (536.5–542.4) | 529.3 (527.5–534.1) | 528.2 (525.6–530.3) | 0.98 (0.98–0.99) |
| p4 | poly100 | wkb | 2,000 | 535.3 (532.8–536.8) | 537.5 (534.7–539.8) | 528.8 (525.1–532.5) | 525.4 (523.3–530.3) | 0.99 (0.98–0.99) |
| p4 | poly1000 | sep | 200 | 5865.7 (5852.3–5928.9) | 5882.9 (5828.9–5945.3) | 5864.4 (5748.6–5997.6) | 5890.6 (5836.0–5960.6) | 1.00 (0.98–1.01) |
| p4 | poly1000 | wkb | 200 | 6140.1 (5884.4–6516.8) | 6113.3 (5784.2–6215.8) | 6095.3 (5737.7–6248.1) | 6003.0 (5864.4–6236.6) | 0.99 (0.88–1.06) |
| p5 | points | sep | 1,000,000 | 239.7 (238.3–242.7) | 235.0 (233.3–237.4) | 168.2 (166.8–169.3) | 152.7 (150.9–154.3) | 0.70 (0.70–0.70) |
| p5 | points | wkb | 1,000,000 | 261.2 (259.6–265.1) | 254.5 (251.8–256.5) | 187.2 (186.0–191.5) | 170.5 (169.7–173.2) | 0.71 (0.71–0.74) |
| p5 | poly10 | sep | 100,000 | 159.9 (158.1–164.2) | 158.1 (155.9–159.6) | 132.0 (129.8–132.5) | 126.2 (124.7–129.6) | 0.82 (0.80–0.83) |
| p5 | poly10 | wkb | 100,000 | 154.7 (152.8–157.5) | 153.9 (151.2–156.4) | 127.0 (125.6–128.1) | 121.5 (120.7–123.5) | 0.82 (0.80–0.83) |
| p5 | poly100 | sep | 100,000 | 1168.7 (1160.0–1198.6) | 1164.2 (1154.9–1169.6) | 1056.2 (1051.2–1079.6) | 1024.4 (1016.6–1036.1) | 0.91 (0.90–0.91) |
| p5 | poly100 | wkb | 100,000 | 1099.5 (1095.0–1119.0) | 1098.4 (1095.9–1129.2) | 999.3 (993.5–1016.2) | 970.5 (965.8–989.0) | 0.91 (0.90–0.91) |
| p5 | poly1000 | sep | 10,000 | 1318.6 (1311.3–1326.6) | 1314.9 (1309.9–1323.1) | 1210.2 (1206.7–1216.4) | 1194.9 (1190.1–1201.5) | 0.92 (0.91–0.93) |
| p5 | poly1000 | wkb | 10,000 | 1232.8 (1230.4–1238.5) | 1232.5 (1224.8–1237.2) | 1154.5 (1149.4–1156.4) | 1137.2 (1135.7–1141.4) | 0.94 (0.93–0.94) |

