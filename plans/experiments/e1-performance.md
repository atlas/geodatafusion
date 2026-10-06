# E1: performance

Decisions: D1 (one loop style), D2 (output encoding), D6 (own the `geo` kernels).
Hypotheses (pre-registered in `plans/hypotheses.md`, unchanged): H1, H2, H6.

Run on 2026-10-05. Experiment code: `experiments/e1-performance/` (a standalone crate, plus
`scripts/`). Raw results: `experiments/e1-performance/results/`. Build output:
`target/experiments/e1`.

## Summary

| Hypothesis | Result | Verdict under the rule |
|---|---|---|
| H1: a WKB-backed `GeometryColumn` costs little next to a typed `downcast_geoarrow_array!` kernel | On native (separated or interleaved) inputs the unified loop costs 4.8–5.3× (ST_X), 5–390× (ST_NPoints, ST_IsEmpty; grows with vertex count), 1.6–3.5× (ST_Area), 1.6–2.9× (ST_Centroid), 1.04–1.10× (ST_Intersects) and 1.00–1.03× (ST_Buffer) in instructions. Wall clock: 3.4–3.7×, 4.9–460×, 1.4–2.7×, 1.4–2.1×, 1.02–1.09×, 0.99–1.02×. On WKB inputs the styles are equal in instructions (0.99–1.09); wall clock 0.93–1.28. | **Refuted.** The cheap accessors *and* two `geo` functions exceed their thresholds, so the rule says: keep a typed kernel for native functions. |
| H2: native outputs make pipelines faster than WKB outputs | With the unified loop, WKB outputs are as fast or faster in every pipeline and input (WKB/native 0.58–1.00 instructions, 0.63–1.00 wall). With typed consumers, native is ≥ 20% faster only for `ST_X(ST_Centroid(points))` (1.46–1.73 instructions, 1.21–1.33 wall); everywhere else WKB is as fast or faster. Output sizes are within 1%, except point outputs (16 B/row native vs 25 B/row WKB). | **Refuted** (at most 1 of 4 pipelines). WKB outputs win. |
| H6: owned `geo` kernels (`GeoColumn` → `geo`) are as fast as `geoarrow-expr-geo`'s | The kernels themselves are equal: on WKB inputs owned/expr-geo is 0.94–1.03, and the owned kernels in the typed loop are 0.99–1.03 of expr-geo on polygons. In the `GeoColumn` shape on native inputs they are 1.6–2.1× (area, centroid), 1.05–1.30× (simplify), 1.03–1.07× (intersects via `relate`) and 1.05–1.90× (intersects via the `Intersects` trait), because of `GeometryColumn`'s WKB conversion. expr-geo also short-circuits point inputs for ST_Area/ST_Simplify (35–120× faster there). | **Refuted as written** (not within 10% end to end). The rule measures D1's conversion cost, not D6; see the comment under H6. |

**What should change in the plan.** The cost isn't the loop shape. It's `geoarrow_array::cast::to_wkb`,
which the prototype `GeometryColumn` calls once per batch: about 225 instructions per coordinate,
more than `geo`'s area algorithm itself (about 180 per coordinate including the `geo` conversion).
With a direct native→WKB writer for points and polygons (a sensitivity variant, `v_`, about 60
lines), the unified loop is within 0.96–1.13× of typed in instructions for `geo` functions with ≥ 100 vertices, 0.94–1.06× in wall
clock for ST_Intersects and ST_Buffer, and is even *faster* in wall clock for ST_Area
and ST_Centroid on polygons (0.61–0.89×), because reading WKB is cheaper than reading native
GeoArrow coordinates through geo-traits. It still costs 3–120× for the cheap native accessors,
since they touch one value per row and the conversion touches every coordinate. So:

- D1: one explicit `for i in 0..number_rows` loop is fine, but `GeometryColumn` must not convert
  native arrays to WKB with `to_wkb` (or must use a fast writer), and the native accessors that
  look at a few values per geometry (ST_X/Y/Z/M, ST_NPoints, ST_IsEmpty, ST_GeometryType,
  ST_NumGeometries, ...) need either a typed fast path or a column whose rows read the native
  array directly (an enum row over native scalars and `Wkb`, not measured here).
- D2: WKB outputs. Native `GeometryType` outputs are slower to build than WKB (dense-union
  builder) and are converted again by every unified consumer; the only win is native *point*
  output read by a typed consumer.
- D6: dropping `geoarrow-expr-geo` costs nothing in the kernels. Owned point-input shortcuts
  (ST_Area of points is 0, ST_Simplify of points is the input) should be kept.

## Method

### Implementations

All variants are `ScalarUDFImpl`s registered in one DataFusion `SessionContext` (one struct,
`BenchUdf`, parameterised by operation, loop style and output encoding, so the plumbing is
identical and only the row loop differs). Per-geometry code is shared between the styles
(`src/kernels.rs`: `x_of`, `npoints`, `is_empty`, `to_geo_value`, `geometry_to_geo`, `to_geos`), so
they differ only in how rows are read.

- **Typed (`t_`)**: `from_arrow_array` + `downcast_geoarrow_array!` into a generic function over
  `GeoArrowArrayAccessor`, iterating `array.iter()`, as in `native/accessors/is_empty.rs`. A
  constant second argument (ST_Intersects) is converted to `geo` once and prepared, as in today's
  `st_intersects`.
- **Unified (`u_`)**: `for i in 0..args.number_rows` over column objects (`src/column.rs`):
  - `GeometryColumn`: the prototype from the task, verbatim apart from error types
    (`from_arrow_array` + `to_wkb::<i32>` once per batch, `read_wkb` per row; zero-copy for WKB
    input).
  - `GeoColumn`: built on `GeometryColumn`; `get(i)` returns `Cow<GeoValue>` (Null/Empty/Geometry,
    G2 §4); a constant is converted once.
  - `GeosColumn`: built on `GeometryColumn`; converts every row to GEOS up front with `to_geos`
    (`CoordSeq::new_from_buffer`, G3 §4), a constant once.
- **Fast-WKB unified (`v_`, sensitivity only)**: the unified code with `GeometryColumn` using
  `fast_to_wkb`, a direct writer for XY Point and Polygon arrays that copies coordinates from the
  buffers. Not part of any verdict.
- **Geometry outputs**: `GeomOut` builds Point (ST_Centroid) or `GeometryType` (everything else)
  native outputs with `coord_type` Separated, or WKB. `GeosGeometryBuilder` (G3 §4) writes GEOS
  results through `WKBWriter` → `read_wkb` → GeoArrow builder, or appends the GEOS WKB bytes
  directly for WKB output.
- **H6 baselines**: geodatafusion's current UDFs, registered with `geodatafusion::register`:
  `st_area`, `st_centroid` and `st_simplify` call `geoarrow-expr-geo`. Today's `st_intersects`
  (`relate.rs`) calls `geoarrow_expr_geo::relate_boolean` for array-array input and its own
  prepared code for a constant. `e_intersects` calls `geoarrow_expr_geo::intersects` (the
  `Intersects` trait). Each is compared with an owned `GeoColumn` kernel using the same algorithm
  (`u_intersectsrelate` for relate, `u_intersects` for the trait). `u_simplify_same` returns the
  input's Polygon type, like expr-geo, instead of `GeometryType`.

Semantics are identical between paired variants (STRICT NULLs, EMPTY via `is_empty`); every query
records a checksum of its result and all paired variants agree (`analyze.py` checks this; 0
mismatches).

### Inputs

Deterministic (splitmix64, fixed seed), no NULLs or EMPTYs:

- `points`: uniform in [0, 1000)².
- `polyN`: star-shaped simple polygons with N vertices (N + 1 coordinates), radius ~1, centres
  uniform in [0, 1000)², N = 10, 100, 1000.
- Encodings: native separated (`sep`), native interleaved (`int`), WKB (`wkb`, `Binary` with the
  GeoArrow extension).
- ST_Intersects' constant: a 100-vertex star polygon of radius ~330 at the centre (about 30% of
  points intersect), passed as `e1_wkb('<WKT>')`, which constant-folds to a WKB scalar (verified:
  the typed variant errors on a non-scalar second argument, and it ran). For array-array
  intersects (H6) the same polygon is materialised as a second column `q`.
- Parameters: ST_Buffer radius 0.1 with 8 quadrant segments; ST_Simplify tolerance 0.1;
  ST_Translate (1, 2).

The pre-registered sizes (1M points, 100k polygons) are used where one run stays within a few
seconds; the expensive functions use fewer rows (`scripts/configs.py`):

| Rows (wall clock) | points | poly10 | poly100 | poly1000 |
|---|--:|--:|--:|--:|
| ST_X, ST_NPoints, ST_IsEmpty | 1,000,000 | 100,000 | 100,000 | 100,000 |
| ST_Area, ST_Centroid, H2 p1–p3, H6 area/centroid/simplify | 1,000,000 | 100,000 | 100,000 | 10,000 |
| ST_Intersects (H1, H6) | 1,000,000 (100,000 array-array) | 100,000 | 20,000 | 2,000 |
| ST_Buffer, H2 p4 | 100,000 | 10,000 | 2,000 | 200 |

Cachegrind used about 10× fewer rows again (the `rows` column in the tables). All results are
compared per row or as ratios, so row counts only matter through per-batch and per-query fixed
costs, which are small (see "Threats").

### Queries and measurement

- `SELECT f(geom) AS r FROM t` (H1, H6), and for H2 `ST_X(ST_Centroid(g))` (p1),
  `ST_Area(ST_Simplify(g, 0.1))` (p2), `ST_AsText(ST_Translate(g, 1, 2))` (p3) and
  `ST_Intersects(ST_Buffer(g, 0.1), <constant polygon>)` (p4), with both functions of a pipeline
  in the same style and the producer's output native (`_n`) or WKB (`_w`). ST_AsText uses
  `wkt::to_wkt::write_geometry` in both styles.
- `t` is a `MemTable` with one partition of 8,192-row batches. `SessionConfig` with
  `target_partitions = 1` and `batch_size = 8192`, on a current-thread Tokio runtime, so a query
  runs on one core (less sensitive to the other jobs on the machine, and deterministic under
  valgrind).
- **End to end**: `ctx.sql(q).await?.collect().await?` (planning, scan, kernels, collect), timed
  with `Instant`.
- **Kernel**: the time inside `invoke_with_args` (`BenchUdf` only; geodatafusion's UDFs aren't
  instrumented).
- **Instructions**: `valgrind --tool=cachegrind --cache-sim=no --instr-at-start=no`, with
  cachegrind client requests (`src/cg.rs`, inline asm, no C dependency) switching counting on
  around the end-to-end query (`e2e`) or around each `invoke_with_args` (`kernel`). Data generation
  and one warm-up query are not counted. Counts repeat to within 0.1% (`t_x`/`u_x`, 3 runs each:
  24,276,363–24,282,768 and 122,422,739–122,424,468), so each configuration ran once.
- **Wall clock**: one process per (input, query group). The variants of a group run interleaved
  in the same process, with a rotating order, 1 warm-up and 5 measured repetitions. Ratios are the
  median of the per-repetition ratios, with min–max. The whole wall-clock matrix ran twice: `wall1`
  (load average 5–9, other agents building) and `wall2` (load average 4.4–4.9). The two agree to
  within a few percent everywhere except the cheapest points queries.

The ratio for H1 is unified / typed; ≤ 1.10 means "≤ 10% overhead". For H2 it is WKB-output /
native-output; "native ≥ 20% faster" is read as WKB/native ≥ 1.20 in wall clock (≥ 1.25 if 20% is
read as a time reduction; no pipeline lands between the two).

## Environment

- AMD Ryzen 7 PRO 250 (8 cores / 16 threads, up to 5.1 GHz), 60 GB RAM, Arch Linux, kernel
  7.2.8. Shared with five other agents running builds and tests; load average 25–31 during the
  first cachegrind batch, 4–9 during the wall-clock runs.
- rustc 1.97.1 (`RUSTUP_TOOLCHAIN=1.97.1`); DataFusion 54.0.0, arrow 58.3.0, geoarrow-array /
  geoarrow-schema / geoarrow-expr-geo 0.8.0, geo 0.31.0, geo-traits 0.3.0, wkb 0.9.1, wkt 0.14.0,
  geos 11.1.1 (geos-sys 2.0.9, feature `v3_11_0`) linked to system GEOS 3.15.0, valgrind 3.25.1.
  `Cargo.lock` is the repository's, copied (same versions).
- Build: `cargo build --release` with Cargo's default release profile (the workspace doesn't
  override it), written out in the crate: `opt-level = 3`, `debug-assertions = false`,
  `overflow-checks = false`, `lto = false`, `codegen-units = 16`. Every variant is in the same
  binary, so they share the profile and codegen settings. All reported numbers are from one build of the
  final source.

## H1: one loop style

### Rule

Adopt the single loop style if the end-to-end overhead is ≤ 10% for every `geo` and GEOS function
and ≤ 2× for the cheap native accessors, and WKB inputs are no slower than the typed path (≤ 5%).
If only the cheap accessors exceed it, adopt the single loop style plus typed fast paths. Otherwise
keep a typed kernel for native functions.

### Results

Unified / typed, end to end, range over inputs (full tables under "Raw data"):

| function | instructions, native inputs | instructions, WKB inputs | wall1 native | wall1 WKB | wall2 native | wall2 WKB | threshold |
|---|--:|--:|--:|--:|--:|--:|--:|
| ST_X | 4.83–5.27 | 0.99 | 3.45–3.65 | 1.27 | 3.42–3.63 | 1.27 | 2× |
| ST_NPoints | 5.84–345 | 0.99–1.00 | 5.17–448 | 1.08–1.13 | 5.02–438 | 1.05–1.15 | 2× |
| ST_IsEmpty | 5.40–390 | 0.99–1.00 | 5.08–460 | 0.95–1.16 | 4.93–434 | 0.98–1.16 | 2× |
| ST_Area | 1.68–3.47 | 1.00–1.09 | 1.40–2.72 | 0.93–1.06 | 1.40–2.66 | 1.01–1.28 | 1.10 |
| ST_Centroid | 1.56–2.88 | 1.00–1.08 | 1.37–2.06 | 1.00–1.49 | 1.37–2.11 | 1.00–1.26 | 1.10 |
| ST_Intersects (constant, prepared) | 1.04–1.10 | 1.00–1.01 | 1.02–1.09 | 1.00–1.02 | 1.02–1.09 | 1.00–1.02 | 1.10 |
| ST_Buffer (GEOS) | 1.00–1.03 | 1.00–1.01 | 0.99–1.02 | 0.99–1.03 | 0.99–1.02 | 0.98–1.03 | 1.10 |

- The overhead on native inputs is the per-batch `to_wkb`: kernel instructions ≈ end-to-end
  instructions in every row of the table (DataFusion's own per-query cost is about 0.1M
  instructions). For ST_NPoints on `poly1000`, typed costs about 650 instructions per row and
  unified about 225,000, i.e. about 225 per coordinate of conversion. Most of it is
  `wkb::writer::write_coord` through `byteorder` into a `GenericByteBuilder`, plus geoarrow's
  per-coordinate `nth_or_panic` and `coord_unchecked` (cg_annotate).
- ST_Intersects and ST_Buffer do enough work per coordinate to hide it (within 10%). ST_Area and
  ST_Centroid don't.
- On WKB inputs the two styles execute the same instructions (± 1%), except ST_Area and
  ST_Centroid on points (1.08–1.09: the `Cow<GeoValue>` per row is visible next to an otherwise
  trivial kernel). Wall clock shows more on cheap WKB queries (ST_X 1.27, ST_Centroid on points
  1.26–1.49) than instruction counts do; this repeated in both passes, so it's real (memory or
  branch behaviour, which `--cache-sim=no` doesn't model), but small in absolute terms (6–19 ms
  per million rows).

Sensitivity, not part of the verdict: with `fast_to_wkb` (`v_`), unified / typed in instructions
is 3.1–3.4 (ST_X), 3.2–122 (ST_NPoints, ST_IsEmpty), 0.96–1.31 (ST_Area, ST_Centroid on polygons;
0.96–1.13 for ≥ 100 vertices), 2.0–2.5 (ST_Area, ST_Centroid on points), 1.00–1.06
(ST_Intersects), 1.00–1.01 (ST_Buffer). In wall clock (wall2): ST_Area and ST_Centroid on polygons
0.61–0.89 (faster than typed), on points 1.6–1.9; ST_Intersects 0.94–1.06; the accessors 2.2–84.

### Verdict

**Refuted.** The cheap accessors exceed 2× (3.4–460× in wall clock) and ST_Area and ST_Centroid
exceed 10% (1.4–2.7× in wall clock, 1.6–3.5× in instructions), on every native input. WKB inputs
pass in instructions except ST_Area/ST_Centroid on points (1.08–1.09) and fail the 5% bar in wall
clock for some cheap queries (ST_X 1.27). Since more than the cheap accessors fail, the rule's
outcome is "keep a typed kernel for native functions".

Comment on the rule: it attributes the overhead to the loop style, but the loop style costs
nothing measurable (WKB inputs, and ST_Intersects/ST_Buffer); the cost is the native→WKB
conversion in the prototype `GeometryColumn`. The rule also doesn't say what happens to the `geo`
functions when they fail, which they do here. With a fast conversion, the `geo` and GEOS functions
would pass on polygons and only the accessors (and `geo` functions on points) would fail, which is
the rule's middle outcome ("single loop style plus typed fast paths").

## H2: output encoding

### Rule

Keep native outputs if they're ≥ 20% faster end to end on at least two of the four pipelines.
Otherwise WKB outputs win.

### Results

WKB-output / native-output, end to end, range over all 12 inputs (> 1 means native is faster):

| pipeline | unified, instructions | unified, wall1 | unified, wall2 | typed, instructions | typed, wall1 | typed, wall2 |
|---|--:|--:|--:|--:|--:|--:|
| p1 `ST_X(ST_Centroid(g))` | 0.83–1.00 | 0.85–1.00 | 0.85–1.00 | 1.00–1.73 | 1.00–1.33 | 1.00–1.32 |
| p2 `ST_Area(ST_Simplify(g, t))` | 0.58–0.99 | 0.63–0.99 | 0.64–0.99 | 0.72–0.99 | 0.68–0.99 | 0.70–0.99 |
| p3 `ST_AsText(ST_Translate(g, 1, 2))` | 0.78–0.94 | 0.79–0.92 | 0.79–0.92 | 0.90–0.97 | 0.90–0.96 | 0.91–1.02 |
| p4 `ST_Intersects(ST_Buffer(p, r), q)` | 0.91–1.00 | 0.88–1.00 | 0.87–1.00 | 0.93–1.00 | 0.93–1.00 | 0.93–1.00 |

- p1 with typed consumers on points is the only case where native wins by ≥ 20% (1.21–1.33 wall,
  1.46–1.73 instructions); on `poly10` it is 1.04–1.08 and on larger polygons 1.00–1.03.
- The unified consumers convert a native intermediate back to WKB, so native intermediates are
  never faster there. With the fast writer (`v_`) p1 becomes 1.00–1.01 and the rest stay
  0.58–1.00.
- `GeometryType` (dense union) outputs cost more to build than WKB, which is why p2 and p3 favour
  WKB even with typed consumers.

Output sizes (buffer bytes, `results/sizes.tsv`; points 100k, poly10 10k, poly100 2k, poly1000 200
rows, separated input):

| producer | points native / WKB | poly10 | poly100 | poly1000 |
|---|--:|--:|--:|--:|
| ST_Centroid (Point output) | 1.60 / 2.50 MB | 0.16 / 0.25 MB | 32.0 / 50.0 kB | 3.2 / 5.0 kB |
| ST_Simplify (`GeometryType`) | 2.10 / 2.50 MB | 1.498 / 1.537 MB | 1.392 / 1.400 MB | 0.397 / 0.398 MB |
| ST_Translate (`GeometryType`) | 2.10 / 2.50 MB | 1.89 / 1.93 MB | 3.258 / 3.266 MB | 3.206 / 3.207 MB |
| ST_Buffer (`GeometryType`) | 54.1 / 54.5 MB | 7.22 / 7.26 MB | 11.46 / 11.46 MB | 1.519 / 1.520 MB |

### Verdict

**Refuted.** Native outputs are ≥ 20% faster on at most one pipeline (p1, typed consumers, point
inputs only), not two. WKB outputs win. With the unified loop (the plan's D1), WKB outputs are
never slower. Sizes don't change this: within 1% for polygon outputs; for point outputs, native is
36% smaller (16 vs 25 bytes per row).

Comment on the rule: "on the H1 inputs" doesn't say whether a pipeline must win on all inputs or
one. The verdict is the same either way.

## H6: own the `geo` kernels

### Rule

Drop `geoarrow-expr-geo` if the owned kernels are within 10% end to end.

### Results

Owned / expr-geo, end to end (instructions; wall clock in the raw tables agrees):

| function | native polygons | WKB polygons | native points | WKB points |
|---|--:|--:|--:|--:|
| ST_Area (`GeoColumn`) | 1.68–2.09 | 0.99–1.02 | 34.5–35.9 | 0.94 |
| ST_Centroid (`GeoColumn`) | 1.56–1.91 | 1.00–1.03 | 2.16–2.22 | 0.96 |
| ST_Simplify (`GeoColumn`, `GeometryType` output) | 1.06–1.30 | 1.00 | 90–95 | 1.02 |
| ST_Simplify (`GeoColumn`, same type as expr-geo) | 1.05–1.19 | 1.00–1.02 | 90–95 | 1.02 |
| ST_Intersects array-array, `relate` (today's st_intersects) | 1.03–1.06 | 0.99 | 1.06–1.07 | 0.99 |
| ST_Intersects array-array, `Intersects` trait (`geoarrow_expr_geo::intersects`) | 1.05–1.37 | 1.00 | 1.72–1.90 | 1.00 |
| ST_Area, owned kernel in the typed loop (`t_area`) | 1.00–1.01 | 0.99–1.00 | | |
| ST_Centroid, owned kernel in the typed loop (`t_centroid`) | 1.00–1.03 | 1.00–1.01 | | |

Wall clock (wall2), native polygons: ST_Area 1.41–1.67, ST_Centroid 1.37–1.57, ST_Simplify
1.05–1.26, intersects/relate 1.02–1.10, intersects/trait 1.07–1.31; WKB polygons 0.98–1.08.

- Where the input is already WKB, so `GeometryColumn` doesn't convert, owned and expr-geo kernels
  are equal (0.94–1.03). The owned kernels in a typed loop are equal on native inputs too
  (0.99–1.03). The `geo` kernel code isn't slower; the `GeoColumn` input path is.
- expr-geo returns zeros for ST_Area of points and copies the array for ST_Simplify of points
  without touching rows; that is the 35–120× on point inputs. An owned kernel can do the same.
- ST_Intersects with a constant (today's own prepared code vs `GeoColumn`): 1.03–1.09 native,
  0.99–1.00 WKB (informative; not expr-geo).
- Today's st_intersects uses DE-9IM `relate` for array-array input, which is 1.4× (polygons with
  ≥ 100 vertices) to 4–13× (points, `poly10`) slower than the `Intersects` trait on these inputs
  (points: 21,340 vs 234,469 instructions per row).

### Verdict

**Refuted as written**: in the planned `GeoColumn` shape, the owned kernels are not within 10% on
native inputs (ST_Area, ST_Centroid, ST_Simplify, trait intersects), and they lose the point-input
shortcuts.

Comment on the rule: as worded ("owned kernels (`GeoColumn` → `geo`)") it measures D1's conversion
cost a second time, not the cost of owning kernels. Measured without that confound (WKB inputs,
or owned kernels in the typed loop) the owned kernels are within 3% everywhere, which supports
dropping `geoarrow-expr-geo` once D1 settles how rows are read, provided the point shortcuts are
kept.

## Threats to validity

- **Shared machine.** Five other agents were building and testing. Wall-clock numbers are from a
  single core per query with interleaved repetitions; both passes agree within a few percent, and
  every verdict is the same in instructions and in wall clock. Instruction counts don't see cache
  misses or branch mispredictions (`--cache-sim=no`), which is why the two metrics differ in
  magnitude (ST_Area native: 1.7–3.5× instructions, 1.4–2.7× time: the WKB conversion has high
  IPC).
- **Codegen sensitivity.** A first cachegrind batch ran partly on an earlier build (before the
  `v_`/`e_` variants were added). The unified conversion path's counts differed by up to 13%
  between the two builds (e.g. `u_area` on `poly100 sep`: 441.7M vs 383.4M instructions; typed
  paths within 1%), with the same conclusions. All reported numbers were re-run on the final
  binary; the earlier batch is kept in `results/cg_first_build.tsv`.
- **The prototype is the treatment.** H1/H6 measure `GeometryColumn` as specified (geoarrow's
  `to_wkb`). A faster conversion changes the `geo` results a lot (see the sensitivity rows), but
  not the accessor results.
- **Scaled row counts.** The expensive functions ran on fewer rows than pre-registered (down to
  200 rows for ST_Buffer on `poly1000`, 50 under cachegrind). Per-query fixed costs are about 0.1M
  instructions, under 1% of every such run.
- **Inputs.** Synthetic star polygons, no holes, no multi-geometries, no NULLs or EMPTYs, XY only,
  and only Point/Polygon arrays (not `GeometryType` or multi types as input). Mixed-type
  `GeometryArray` inputs go through the same `to_wkb` and are likely no cheaper.
- **Single partition.** `target_partitions = 1`. With parallelism, both styles scale the same
  way, so ratios should hold; absolute times won't.
- **H2 typed consumers** read `GeometryType` intermediates; a plan that kept typed kernels would
  also produce `GeometryType` (README "Geometry output type"), so this matches.

## Reproduce

```sh
cd experiments/e1-performance
export RUSTUP_TOOLCHAIN=1.97.1
export CARGO_TARGET_DIR=$PWD/../../target/experiments/e1
cargo build --release

# Instruction counts (deterministic; parallel is fine). Appends to results/cg.tsv.
python3 scripts/run_cg.py --jobs 8 H1 H2 H6
python3 scripts/run_cg.py --jobs 8 H1v   # fast-WKB sensitivity
python3 scripts/run_cg.py --jobs 8 H2v

# Wall clock, 1 warm-up + 5 interleaved repetitions. Appends to results/<tag>.tsv.
python3 scripts/run_wall.py --tag wall1 H1 H2 H6
python3 scripts/run_wall.py --tag wall2 H1 H2 H6

# Output sizes (H2).
B=$CARGO_TARGET_DIR/release/e1-performance
for d in points:100000 poly10:10000 poly100:2000 poly1000:200; do
  $B sizes ${d%%:*} sep ${d##*:}
done > results/sizes.tsv

# Tables.
python3 scripts/analyze.py wall1 wall2
SUMMARY=1 python3 scripts/analyze.py wall1 wall2

# One query by hand:
$B run poly100 sep 100000 5 t_area u_area v_area
valgrind --tool=cachegrind --cache-sim=no --instr-at-start=no \
  $B cg e2e poly100 sep 10000 u_area
```

Query ids: `<style>_<function>[_aa|_same]` with style `t` (typed), `u` (unified), `v` (unified,
fast WKB), `g` (geodatafusion today), `e` (`geoarrow_expr_geo::intersects`); `_aa` takes the
array column `q` as the second ST_Intersects argument. H2 ids are `h2_<p1..p4>_<style>_<n|w>`.
`e1 list` prints examples.

## Raw data

Per-row instruction counts and wall-clock milliseconds per query (all rows of the input).
#### H1: instruction counts per row (cachegrind)

Overhead = unified / typed. `e2e` is the whole query (planning, MemTable scan, kernels, collect), `kernel` is `invoke_with_args` only.

| function | input | enc | rows | typed e2e | unified e2e | **e2e ratio** | typed kernel | unified kernel | kernel ratio | fast-WKB e2e | fast-WKB ratio |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| x | points | sep | 100,000 | 244 | 1,285 | **5.27** | 242 | 1,284 | 5.30 | 832 | 3.41 |
| x | points | int | 100,000 | 271 | 1,309 | **4.83** | 270 | 1,308 | 4.84 | 840 | 3.09 |
| x | points | wkb | 100,000 | 691 | 685 | **0.99** | 690 | 684 | 0.99 |  |  |
| npoints | points | sep | 100,000 | 210 | 1,312 | **6.25** | 208 | 1,310 | 6.29 | 862 | 4.11 |
| npoints | points | int | 100,000 | 229 | 1,340 | **5.84** | 228 | 1,339 | 5.87 | 870 | 3.79 |
| npoints | points | wkb | 100,000 | 721 | 715 | **0.99** | 720 | 714 | 0.99 |  |  |
| npoints | poly10 | sep | 10,000 | 665 | 4,738 | **7.13** | 657 | 4,730 | 7.20 | 2,134 | 3.21 |
| npoints | poly10 | int | 10,000 | 664 | 4,962 | **7.47** | 656 | 4,951 | 7.55 | 2,213 | 3.33 |
| npoints | poly10 | wkb | 10,000 | 1,240 | 1,235 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| npoints | poly100 | sep | 10,000 | 664 | 23,314 | **35.12** | 656 | 23,309 | 35.55 | 7,892 | 11.89 |
| npoints | poly100 | int | 10,000 | 663 | 25,337 | **38.22** | 655 | 25,330 | 38.67 | 8,511 | 12.84 |
| npoints | poly100 | wkb | 10,000 | 1,240 | 1,234 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| npoints | poly1000 | sep | 10,000 | 664 | 208,710 | **314.40** | 656 | 208,703 | 318.12 | 65,493 | 98.66 |
| npoints | poly1000 | int | 10,000 | 663 | 228,733 | **345.20** | 654 | 228,724 | 349.61 | 71,511 | 107.92 |
| npoints | poly1000 | wkb | 10,000 | 1,239 | 1,234 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| isempty | points | sep | 100,000 | 230 | 1,324 | **5.76** | 229 | 1,322 | 5.79 | 870 | 3.79 |
| isempty | points | int | 100,000 | 249 | 1,348 | **5.40** | 248 | 1,346 | 5.43 | 878 | 3.52 |
| isempty | points | wkb | 100,000 | 729 | 723 | **0.99** | 728 | 722 | 0.99 |  |  |
| isempty | poly10 | sep | 10,000 | 589 | 4,739 | **8.04** | 581 | 4,730 | 8.14 | 2,134 | 3.62 |
| isempty | poly10 | int | 10,000 | 588 | 4,961 | **8.44** | 580 | 4,953 | 8.54 | 2,212 | 3.77 |
| isempty | poly10 | wkb | 10,000 | 1,240 | 1,235 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| isempty | poly100 | sep | 10,000 | 588 | 23,314 | **39.66** | 580 | 23,307 | 40.18 | 7,893 | 13.43 |
| isempty | poly100 | int | 10,000 | 587 | 25,337 | **43.18** | 579 | 25,329 | 43.74 | 8,511 | 14.51 |
| isempty | poly100 | wkb | 10,000 | 1,240 | 1,234 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| isempty | poly1000 | sep | 10,000 | 588 | 208,710 | **354.93** | 580 | 208,702 | 359.87 | 65,493 | 111.38 |
| isempty | poly1000 | int | 10,000 | 586 | 228,733 | **390.13** | 579 | 228,725 | 395.28 | 71,511 | 121.97 |
| isempty | poly1000 | wkb | 10,000 | 1,239 | 1,234 | **1.00** | 1,232 | 1,226 | 1.00 |  |  |
| area | points | sep | 100,000 | 476 | 1,651 | **3.47** | 474 | 1,650 | 3.48 | 1,198 | 2.52 |
| area | points | int | 100,000 | 533 | 1,675 | **3.14** | 532 | 1,674 | 3.15 | 1,206 | 2.26 |
| area | points | wkb | 100,000 | 961 | 1,051 | **1.09** | 960 | 1,050 | 1.09 |  |  |
| area | poly10 | sep | 10,000 | 3,409 | 7,060 | **2.07** | 3,401 | 7,053 | 2.07 | 4,456 | 1.31 |
| area | poly10 | int | 10,000 | 3,782 | 7,282 | **1.93** | 3,774 | 7,275 | 1.93 | 4,535 | 1.20 |
| area | poly10 | wkb | 10,000 | 3,466 | 3,557 | **1.03** | 3,458 | 3,548 | 1.03 |  |  |
| area | poly100 | sep | 10,000 | 20,295 | 38,358 | **1.89** | 20,271 | 38,290 | 1.89 | 22,868 | 1.13 |
| area | poly100 | int | 10,000 | 23,693 | 40,321 | **1.70** | 23,684 | 40,312 | 1.70 | 23,558 | 0.99 |
| area | poly100 | wkb | 10,000 | 16,148 | 16,282 | **1.01** | 16,130 | 16,253 | 1.01 |  |  |
| area | poly1000 | sep | 1,000 | 183,358 | 345,720 | **1.89** | 183,279 | 345,534 | 1.89 | 202,500 | 1.10 |
| area | poly1000 | int | 1,000 | 217,378 | 365,729 | **1.68** | 217,310 | 365,657 | 1.68 | 208,507 | 0.96 |
| area | poly1000 | wkb | 1,000 | 137,980 | 138,073 | **1.00** | 137,738 | 138,000 | 1.00 |  |  |
| centroid | points | sep | 100,000 | 627 | 1,808 | **2.88** | 626 | 1,806 | 2.89 | 1,352 | 2.15 |
| centroid | points | int | 100,000 | 685 | 1,833 | **2.68** | 683 | 1,832 | 2.68 | 1,359 | 1.98 |
| centroid | points | wkb | 100,000 | 1,118 | 1,205 | **1.08** | 1,117 | 1,203 | 1.08 |  |  |
| centroid | poly10 | sep | 10,000 | 4,242 | 7,888 | **1.86** | 4,234 | 7,881 | 1.86 | 5,285 | 1.25 |
| centroid | poly10 | int | 10,000 | 4,615 | 8,111 | **1.76** | 4,607 | 8,102 | 1.76 | 5,364 | 1.16 |
| centroid | poly10 | wkb | 10,000 | 4,300 | 4,386 | **1.02** | 4,291 | 4,378 | 1.02 |  |  |
| centroid | poly100 | sep | 10,000 | 25,238 | 43,298 | **1.72** | 25,186 | 43,300 | 1.72 | 27,873 | 1.10 |
| centroid | poly100 | int | 10,000 | 28,696 | 45,333 | **1.58** | 28,624 | 45,310 | 1.58 | 28,507 | 0.99 |
| centroid | poly100 | wkb | 10,000 | 21,133 | 21,219 | **1.00** | 21,130 | 21,211 | 1.00 |  |  |
| centroid | poly1000 | sep | 1,000 | 230,253 | 392,276 | **1.70** | 230,179 | 392,199 | 1.70 | 249,056 | 1.08 |
| centroid | poly1000 | int | 1,000 | 264,276 | 412,276 | **1.56** | 264,198 | 412,212 | 1.56 | 255,062 | 0.97 |
| centroid | poly1000 | wkb | 1,000 | 184,711 | 184,798 | **1.00** | 184,636 | 184,722 | 1.00 |  |  |
| intersects | points | sep | 100,000 | 11,633 | 12,812 | **1.10** | 11,636 | 12,806 | 1.10 | 12,163 | 1.05 |
| intersects | points | int | 100,000 | 11,728 | 12,862 | **1.10** | 11,707 | 12,817 | 1.09 | 12,381 | 1.06 |
| intersects | points | wkb | 100,000 | 12,051 | 12,159 | **1.01** | 12,061 | 12,108 | 1.00 |  |  |
| intersects | poly10 | sep | 10,000 | 42,954 | 46,794 | **1.09** | 42,993 | 46,998 | 1.09 | 44,164 | 1.03 |
| intersects | poly10 | int | 10,000 | 43,654 | 47,004 | **1.08** | 43,792 | 47,105 | 1.08 | 44,300 | 1.01 |
| intersects | poly10 | wkb | 10,000 | 43,202 | 43,287 | **1.00** | 42,946 | 43,163 | 1.01 |  |  |
| intersects | poly100 | sep | 2,000 | 262,765 | 281,493 | **1.07** | 262,174 | 280,937 | 1.07 | 264,683 | 1.01 |
| intersects | poly100 | int | 2,000 | 265,327 | 282,068 | **1.06** | 265,908 | 282,759 | 1.06 | 265,948 | 1.00 |
| intersects | poly100 | wkb | 2,000 | 258,813 | 257,923 | **1.00** | 259,299 | 257,694 | 0.99 |  |  |
| intersects | poly1000 | sep | 500 | 4,055,438 | 4,218,917 | **1.04** | 4,060,316 | 4,219,377 | 1.04 | 4,077,067 | 1.01 |
| intersects | poly1000 | int | 500 | 4,091,983 | 4,237,654 | **1.04** | 4,090,976 | 4,238,912 | 1.04 | 4,081,863 | 1.00 |
| intersects | poly1000 | wkb | 500 | 4,014,158 | 4,012,481 | **1.00** | 4,013,061 | 4,012,044 | 1.00 |  |  |
| buffer | points | sep | 10,000 | 157,049 | 158,883 | **1.01** | 156,975 | 158,808 | 1.01 | 158,440 | 1.01 |
| buffer | points | int | 10,000 | 156,731 | 158,915 | **1.01** | 157,007 | 158,839 | 1.01 | 158,548 | 1.01 |
| buffer | points | wkb | 10,000 | 157,249 | 158,978 | **1.01** | 157,174 | 158,903 | 1.01 |  |  |
| buffer | poly10 | sep | 1,000 | 252,740 | 259,061 | **1.03** | 252,302 | 258,623 | 1.03 | 256,524 | 1.01 |
| buffer | poly10 | int | 1,000 | 253,301 | 259,388 | **1.02** | 252,875 | 258,951 | 1.02 | 256,613 | 1.01 |
| buffer | poly10 | wkb | 1,000 | 252,289 | 255,479 | **1.01** | 251,850 | 255,040 | 1.01 |  |  |
| buffer | poly100 | sep | 200 | 5,434,456 | 5,464,854 | **1.01** | 5,432,280 | 5,462,650 | 1.01 | 5,449,263 | 1.00 |
| buffer | poly100 | int | 200 | 5,439,974 | 5,466,375 | **1.00** | 5,437,810 | 5,464,150 | 1.00 | 5,449,251 | 1.00 |
| buffer | poly100 | wkb | 200 | 5,430,528 | 5,436,308 | **1.00** | 5,428,338 | 5,432,884 | 1.00 |  |  |
| buffer | poly1000 | sep | 50 | 494,699,990 | 494,837,545 | **1.00** | 494,691,352 | 494,832,873 | 1.00 | 494,694,507 | 1.00 |
| buffer | poly1000 | int | 50 | 494,727,298 | 494,864,876 | **1.00** | 494,730,593 | 494,855,946 | 1.00 | 494,695,251 | 1.00 |
| buffer | poly1000 | wkb | 50 | 494,650,056 | 494,655,034 | **1.00** | 494,611,589 | 494,646,267 | 1.00 |  |  |

#### H1: wall clock (wall1)

Median of 5 interleaved repetitions; ratio = median of the per-repetition unified/typed ratios, with min–max.

| function | input | enc | rows | typed ms | unified ms | **e2e ratio** (min–max) | typed kernel ms | unified kernel ms | kernel ratio | fast-WKB ratio |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|
| x | points | sep | 1,000,000 | 14.3 | 51.9 | **3.65** (3.51–3.81) | 13.7 | 51.3 | 3.75 | 2.29 |
| x | points | int | 1,000,000 | 15.4 | 53.1 | **3.45** (3.16–3.46) | 14.9 | 52.4 | 3.52 | 2.13 |
| x | points | wkb | 1,000,000 | 23.0 | 29.1 | **1.27** (1.25–1.27) | 22.4 | 28.4 | 1.27 |  |
| npoints | points | sep | 1,000,000 | 9.9 | 51.3 | **5.21** (5.12–5.27) | 9.4 | 50.8 | 5.41 | 3.06 |
| npoints | points | int | 1,000,000 | 9.8 | 51.2 | **5.17** (5.03–5.25) | 9.4 | 50.6 | 5.37 | 3.11 |
| npoints | points | wkb | 1,000,000 | 23.1 | 26.2 | **1.13** (1.12–1.14) | 22.6 | 25.7 | 1.14 |  |
| npoints | poly10 | sep | 100,000 | 2.5 | 19.9 | **7.84** (7.59–7.90) | 2.3 | 19.5 | 8.56 | 2.49 |
| npoints | poly10 | int | 100,000 | 2.6 | 21.5 | **8.27** (7.75–8.35) | 2.3 | 21.3 | 9.43 | 2.37 |
| npoints | poly10 | wkb | 100,000 | 3.7 | 4.1 | **1.11** (1.05–1.14) | 3.4 | 3.8 | 1.10 |  |
| npoints | poly100 | sep | 100,000 | 2.8 | 116.7 | **41.43** (40.34–42.15) | 2.3 | 116.2 | 51.00 | 8.04 |
| npoints | poly100 | int | 100,000 | 2.9 | 130.3 | **45.40** (45.06–46.08) | 2.3 | 129.6 | 56.56 | 7.41 |
| npoints | poly100 | wkb | 100,000 | 4.4 | 4.7 | **1.09** (1.04–1.20) | 4.2 | 4.5 | 1.09 |  |
| npoints | poly1000 | sep | 100,000 | 2.9 | 1145.3 | **398.66** (384.34–402.99) | 2.4 | 1144.9 | 486.46 | 80.25 |
| npoints | poly1000 | int | 100,000 | 2.8 | 1271.7 | **447.61** (428.94–452.28) | 2.3 | 1270.9 | 546.61 | 76.69 |
| npoints | poly1000 | wkb | 100,000 | 5.2 | 5.7 | **1.08** (1.04–1.25) | 4.9 | 5.4 | 1.09 |  |
| isempty | points | sep | 1,000,000 | 8.6 | 43.6 | **5.08** (4.93–5.22) | 8.2 | 43.2 | 5.26 | 3.32 |
| isempty | points | int | 1,000,000 | 8.8 | 49.6 | **5.62** (5.04–5.90) | 8.3 | 49.1 | 5.93 | 3.31 |
| isempty | points | wkb | 1,000,000 | 21.8 | 25.5 | **1.16** (1.16–1.18) | 21.4 | 25.0 | 1.17 |  |
| isempty | poly10 | sep | 100,000 | 2.5 | 20.3 | **8.18** (7.47–8.35) | 2.2 | 20.0 | 9.11 | 2.51 |
| isempty | poly10 | int | 100,000 | 2.5 | 21.8 | **8.86** (8.68–9.04) | 2.2 | 21.5 | 9.77 | 2.51 |
| isempty | poly10 | wkb | 100,000 | 3.8 | 4.1 | **1.09** (1.08–1.11) | 3.5 | 3.9 | 1.11 |  |
| isempty | poly100 | sep | 100,000 | 2.8 | 118.1 | **43.04** (41.63–43.34) | 2.2 | 117.8 | 53.69 | 8.18 |
| isempty | poly100 | int | 100,000 | 2.8 | 131.6 | **46.29** (45.94–47.26) | 2.3 | 131.1 | 58.18 | 7.58 |
| isempty | poly100 | wkb | 100,000 | 4.9 | 4.7 | **0.97** (0.91–1.06) | 4.6 | 4.4 | 0.97 |  |
| isempty | poly1000 | sep | 100,000 | 2.8 | 1145.2 | **412.99** (398.70–420.31) | 2.3 | 1144.4 | 505.89 | 82.76 |
| isempty | poly1000 | int | 100,000 | 2.8 | 1290.2 | **460.34** (459.29–477.44) | 2.3 | 1289.4 | 568.87 | 79.08 |
| isempty | poly1000 | wkb | 100,000 | 5.3 | 5.0 | **0.95** (0.84–1.04) | 5.0 | 4.8 | 0.95 |  |
| area | points | sep | 1,000,000 | 25.4 | 68.9 | **2.72** (2.70–2.73) | 24.7 | 68.2 | 2.76 | 1.94 |
| area | points | int | 1,000,000 | 26.5 | 69.2 | **2.59** (2.57–2.61) | 25.9 | 68.4 | 2.63 | 1.86 |
| area | points | wkb | 1,000,000 | 48.8 | 45.4 | **0.93** (0.93–0.93) | 48.1 | 44.7 | 0.93 |  |
| area | poly10 | sep | 100,000 | 19.1 | 30.0 | **1.58** (1.46–1.61) | 18.6 | 29.6 | 1.59 | 0.84 |
| area | poly10 | int | 100,000 | 20.0 | 31.7 | **1.58** (1.57–1.58) | 19.7 | 31.2 | 1.59 | 0.79 |
| area | poly10 | wkb | 100,000 | 12.8 | 13.6 | **1.06** (1.06–1.06) | 12.6 | 13.3 | 1.06 |  |
| area | poly100 | sep | 100,000 | 124.9 | 180.4 | **1.44** (1.44–1.45) | 124.2 | 179.7 | 1.44 | 0.68 |
| area | poly100 | int | 100,000 | 138.9 | 194.2 | **1.40** (1.39–1.41) | 138.2 | 193.5 | 1.40 | 0.61 |
| area | poly100 | wkb | 100,000 | 66.9 | 68.1 | **1.02** (1.01–1.02) | 66.3 | 67.5 | 1.02 |  |
| area | poly1000 | sep | 10,000 | 118.6 | 172.7 | **1.45** (1.44–1.47) | 118.0 | 172.1 | 1.46 | 0.68 |
| area | poly1000 | int | 10,000 | 131.3 | 184.8 | **1.41** (1.40–1.41) | 130.7 | 184.3 | 1.41 | 0.61 |
| area | poly1000 | wkb | 10,000 | 58.9 | 59.0 | **1.00** (1.00–1.00) | 58.3 | 58.4 | 1.00 |  |
| centroid | points | sep | 1,000,000 | 33.3 | 68.6 | **2.06** (2.04–2.07) | 32.6 | 68.0 | 2.09 | 1.63 |
| centroid | points | int | 1,000,000 | 34.1 | 69.9 | **2.05** (2.02–2.06) | 33.4 | 69.3 | 2.07 | 1.58 |
| centroid | points | wkb | 1,000,000 | 39.2 | 58.2 | **1.49** (1.48–1.52) | 38.5 | 57.6 | 1.50 |  |
| centroid | poly10 | sep | 100,000 | 21.7 | 33.0 | **1.52** (1.51–1.54) | 21.3 | 32.7 | 1.53 | 0.88 |
| centroid | poly10 | int | 100,000 | 23.1 | 34.6 | **1.50** (1.46–1.51) | 22.7 | 34.2 | 1.51 | 0.82 |
| centroid | poly10 | wkb | 100,000 | 16.5 | 16.9 | **1.02** (1.01–1.04) | 16.2 | 16.6 | 1.02 |  |
| centroid | poly100 | sep | 100,000 | 138.0 | 193.2 | **1.40** (1.39–1.44) | 137.3 | 192.5 | 1.40 | 0.71 |
| centroid | poly100 | int | 100,000 | 151.2 | 207.2 | **1.37** (1.36–1.38) | 150.5 | 206.5 | 1.37 | 0.64 |
| centroid | poly100 | wkb | 100,000 | 79.5 | 80.0 | **1.01** (0.99–1.01) | 78.9 | 79.4 | 1.01 |  |
| centroid | poly1000 | sep | 10,000 | 129.0 | 183.7 | **1.42** (1.41–1.43) | 128.4 | 183.1 | 1.42 | 0.71 |
| centroid | poly1000 | int | 10,000 | 142.6 | 196.0 | **1.38** (1.37–1.38) | 142.0 | 195.5 | 1.38 | 0.64 |
| centroid | poly1000 | wkb | 10,000 | 69.7 | 70.0 | **1.00** (0.99–1.01) | 69.2 | 69.5 | 1.00 |  |
| intersects | points | sep | 1,000,000 | 433.7 | 472.0 | **1.09** (1.07–1.10) | 432.6 | 470.8 | 1.09 | 1.06 |
| intersects | points | int | 1,000,000 | 432.9 | 476.2 | **1.09** (1.09–1.12) | 431.7 | 475.0 | 1.09 | 1.06 |
| intersects | points | wkb | 1,000,000 | 446.1 | 454.7 | **1.02** (1.01–1.02) | 444.9 | 453.6 | 1.02 |  |
| intersects | poly10 | sep | 100,000 | 151.5 | 161.5 | **1.07** (1.06–1.08) | 150.4 | 160.5 | 1.07 | 0.98 |
| intersects | poly10 | int | 100,000 | 152.8 | 163.6 | **1.07** (1.07–1.11) | 151.8 | 162.6 | 1.07 | 0.97 |
| intersects | poly10 | wkb | 100,000 | 148.6 | 149.0 | **1.00** (1.00–1.02) | 147.6 | 148.1 | 1.00 |  |
| intersects | poly100 | sep | 20,000 | 212.6 | 225.4 | **1.06** (1.05–1.07) | 211.6 | 224.4 | 1.06 | 0.96 |
| intersects | poly100 | int | 20,000 | 215.8 | 225.7 | **1.04** (1.03–1.06) | 214.8 | 224.7 | 1.04 | 0.95 |
| intersects | poly100 | wkb | 20,000 | 200.0 | 200.7 | **1.00** (0.97–1.01) | 199.1 | 199.8 | 1.00 |  |
| intersects | poly1000 | sep | 2,000 | 429.6 | 437.9 | **1.02** (1.02–1.03) | 428.7 | 437.0 | 1.02 | 0.98 |
| intersects | poly1000 | int | 2,000 | 434.0 | 443.3 | **1.02** (1.02–1.03) | 433.1 | 442.4 | 1.02 | 0.97 |
| intersects | poly1000 | wkb | 2,000 | 418.8 | 417.9 | **1.00** (0.99–1.00) | 417.9 | 417.0 | 1.00 |  |
| buffer | points | sep | 100,000 | 462.3 | 462.2 | **1.00** (0.99–1.01) | 460.7 | 460.7 | 1.00 | 0.99 |
| buffer | points | int | 100,000 | 471.5 | 480.0 | **1.02** (1.00–1.04) | 470.0 | 478.4 | 1.02 | 1.01 |
| buffer | points | wkb | 100,000 | 467.2 | 466.3 | **1.00** (1.00–1.01) | 465.7 | 464.8 | 1.00 |  |
| buffer | poly10 | sep | 10,000 | 76.8 | 77.8 | **1.01** (0.98–1.03) | 75.8 | 76.8 | 1.01 | 1.00 |
| buffer | poly10 | int | 10,000 | 75.7 | 77.0 | **1.02** (1.01–1.04) | 74.7 | 76.0 | 1.02 | 1.00 |
| buffer | poly10 | wkb | 10,000 | 74.8 | 76.7 | **1.03** (1.02–1.04) | 73.8 | 75.7 | 1.03 |  |
| buffer | poly100 | sep | 2,000 | 459.9 | 457.3 | **0.99** (0.99–1.00) | 459.0 | 456.3 | 0.99 | 0.99 |
| buffer | poly100 | int | 2,000 | 462.3 | 459.2 | **0.99** (0.98–1.01) | 461.4 | 458.2 | 0.99 | 0.99 |
| buffer | poly100 | wkb | 2,000 | 464.1 | 457.0 | **0.99** (0.98–1.00) | 462.3 | 456.0 | 0.99 |  |
| buffer | poly1000 | sep | 200 | 5327.3 | 5291.9 | **0.99** (0.97–1.00) | 5326.3 | 5290.9 | 0.99 | 0.99 |
| buffer | poly1000 | int | 200 | 5746.4 | 5555.0 | **1.01** (0.95–1.08) | 5745.4 | 5554.0 | 1.01 | 0.98 |
| buffer | poly1000 | wkb | 200 | 5346.5 | 5599.9 | **1.02** (0.99–1.08) | 5345.5 | 5598.9 | 1.02 |  |

#### H1: wall clock (wall2)

Median of 5 interleaved repetitions; ratio = median of the per-repetition unified/typed ratios, with min–max.

| function | input | enc | rows | typed ms | unified ms | **e2e ratio** (min–max) | typed kernel ms | unified kernel ms | kernel ratio | fast-WKB ratio |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|
| x | points | sep | 1,000,000 | 14.2 | 51.4 | **3.63** (3.60–3.68) | 13.6 | 50.8 | 3.73 | 2.30 |
| x | points | int | 1,000,000 | 15.2 | 52.0 | **3.42** (3.39–3.46) | 14.6 | 51.4 | 3.52 | 2.16 |
| x | points | wkb | 1,000,000 | 22.6 | 28.6 | **1.27** (1.26–1.27) | 22.1 | 28.0 | 1.27 |  |
| npoints | points | sep | 1,000,000 | 9.9 | 50.1 | **5.02** (5.00–5.10) | 9.3 | 49.4 | 5.29 | 3.05 |
| npoints | points | int | 1,000,000 | 9.9 | 50.5 | **5.14** (5.05–5.18) | 9.3 | 50.0 | 5.34 | 3.06 |
| npoints | points | wkb | 1,000,000 | 22.8 | 26.2 | **1.15** (1.13–1.15) | 22.3 | 25.6 | 1.14 |  |
| npoints | poly10 | sep | 100,000 | 2.5 | 19.9 | **7.89** (7.28–8.06) | 2.3 | 19.6 | 8.69 | 2.41 |
| npoints | poly10 | int | 100,000 | 2.5 | 21.4 | **8.49** (8.08–8.64) | 2.3 | 21.2 | 9.40 | 2.41 |
| npoints | poly10 | wkb | 100,000 | 3.7 | 4.0 | **1.09** (1.08–1.10) | 3.4 | 3.8 | 1.10 |  |
| npoints | poly100 | sep | 100,000 | 2.8 | 116.4 | **41.65** (40.61–41.80) | 2.3 | 116.0 | 50.79 | 8.04 |
| npoints | poly100 | int | 100,000 | 3.0 | 131.8 | **44.65** (41.80–46.32) | 2.3 | 131.2 | 56.43 | 7.32 |
| npoints | poly100 | wkb | 100,000 | 4.7 | 5.0 | **1.07** (0.99–1.11) | 4.5 | 4.8 | 1.07 |  |
| npoints | poly1000 | sep | 100,000 | 2.8 | 1139.1 | **396.09** (391.17–404.95) | 2.3 | 1138.4 | 485.63 | 79.29 |
| npoints | poly1000 | int | 100,000 | 2.9 | 1264.0 | **437.98** (428.33–447.47) | 2.4 | 1263.4 | 533.08 | 76.41 |
| npoints | poly1000 | wkb | 100,000 | 4.8 | 5.1 | **1.05** (1.02–1.07) | 4.6 | 4.9 | 1.05 |  |
| isempty | points | sep | 1,000,000 | 8.7 | 43.0 | **4.93** (4.51–5.05) | 8.2 | 42.5 | 5.22 | 3.31 |
| isempty | points | int | 1,000,000 | 8.7 | 46.6 | **5.44** (5.03–5.75) | 8.3 | 46.1 | 5.61 | 3.33 |
| isempty | points | wkb | 1,000,000 | 21.7 | 25.1 | **1.16** (1.15–1.17) | 21.3 | 24.7 | 1.16 |  |
| isempty | poly10 | sep | 100,000 | 2.6 | 20.7 | **8.04** (7.70–8.34) | 2.2 | 20.4 | 9.20 | 2.60 |
| isempty | poly10 | int | 100,000 | 2.6 | 24.3 | **9.53** (9.11–9.63) | 2.2 | 24.0 | 10.92 | 2.99 |
| isempty | poly10 | wkb | 100,000 | 3.7 | 4.1 | **1.09** (1.06–1.11) | 3.5 | 3.8 | 1.10 |  |
| isempty | poly100 | sep | 100,000 | 2.8 | 118.0 | **42.33** (42.19–43.11) | 2.2 | 117.3 | 52.86 | 8.18 |
| isempty | poly100 | int | 100,000 | 2.8 | 131.7 | **46.35** (42.56–48.53) | 2.2 | 131.3 | 58.01 | 7.58 |
| isempty | poly100 | wkb | 100,000 | 4.3 | 4.5 | **1.05** (0.98–1.06) | 4.1 | 4.3 | 1.05 |  |
| isempty | poly1000 | sep | 100,000 | 2.8 | 1142.3 | **413.57** (396.29–421.68) | 2.3 | 1141.9 | 506.37 | 83.66 |
| isempty | poly1000 | int | 100,000 | 2.9 | 1274.4 | **433.90** (430.15–444.59) | 2.4 | 1273.9 | 528.37 | 75.47 |
| isempty | poly1000 | wkb | 100,000 | 5.1 | 4.9 | **0.98** (0.96–1.03) | 4.8 | 4.7 | 0.98 |  |
| area | points | sep | 1,000,000 | 25.4 | 67.7 | **2.66** (2.65–2.73) | 24.8 | 67.0 | 2.71 | 1.91 |
| area | points | int | 1,000,000 | 26.5 | 69.3 | **2.62** (2.60–2.67) | 25.8 | 68.7 | 2.66 | 1.87 |
| area | points | wkb | 1,000,000 | 34.9 | 44.9 | **1.28** (1.27–1.29) | 34.4 | 44.2 | 1.29 |  |
| area | poly10 | sep | 100,000 | 18.4 | 29.7 | **1.61** (1.54–1.62) | 18.1 | 29.3 | 1.61 | 0.86 |
| area | poly10 | int | 100,000 | 19.9 | 31.6 | **1.58** (1.58–1.61) | 19.6 | 31.2 | 1.59 | 0.79 |
| area | poly10 | wkb | 100,000 | 12.9 | 13.6 | **1.05** (1.05–1.06) | 12.6 | 13.3 | 1.06 |  |
| area | poly100 | sep | 100,000 | 125.1 | 179.3 | **1.43** (1.42–1.44) | 124.5 | 178.6 | 1.44 | 0.68 |
| area | poly100 | int | 100,000 | 137.3 | 191.8 | **1.40** (1.39–1.40) | 136.6 | 191.2 | 1.40 | 0.61 |
| area | poly100 | wkb | 100,000 | 66.1 | 67.3 | **1.02** (1.01–1.02) | 65.5 | 66.7 | 1.02 |  |
| area | poly1000 | sep | 10,000 | 117.3 | 171.1 | **1.46** (1.45–1.48) | 116.8 | 170.6 | 1.46 | 0.68 |
| area | poly1000 | int | 10,000 | 130.7 | 185.7 | **1.42** (1.34–1.42) | 130.1 | 185.1 | 1.42 | 0.61 |
| area | poly1000 | wkb | 10,000 | 58.8 | 59.6 | **1.01** (0.99–1.02) | 58.2 | 59.0 | 1.01 |  |
| centroid | points | sep | 1,000,000 | 33.0 | 68.1 | **2.08** (2.06–2.08) | 32.3 | 67.4 | 2.09 | 1.61 |
| centroid | points | int | 1,000,000 | 33.5 | 70.6 | **2.11** (2.09–2.12) | 32.9 | 70.0 | 2.13 | 1.60 |
| centroid | points | wkb | 1,000,000 | 39.1 | 49.3 | **1.26** (1.25–1.26) | 38.6 | 48.7 | 1.26 |  |
| centroid | poly10 | sep | 100,000 | 21.3 | 32.6 | **1.53** (1.49–1.55) | 20.9 | 32.3 | 1.54 | 0.89 |
| centroid | poly10 | int | 100,000 | 22.9 | 34.3 | **1.50** (1.48–1.51) | 22.5 | 33.9 | 1.51 | 0.83 |
| centroid | poly10 | wkb | 100,000 | 16.5 | 16.7 | **1.01** (0.97–1.02) | 16.2 | 16.4 | 1.01 |  |
| centroid | poly100 | sep | 100,000 | 137.1 | 192.5 | **1.41** (1.40–1.45) | 136.4 | 191.9 | 1.41 | 0.71 |
| centroid | poly100 | int | 100,000 | 150.3 | 205.8 | **1.37** (1.36–1.37) | 149.6 | 205.1 | 1.37 | 0.64 |
| centroid | poly100 | wkb | 100,000 | 78.9 | 80.2 | **1.01** (0.98–1.05) | 78.3 | 79.6 | 1.01 |  |
| centroid | poly1000 | sep | 10,000 | 127.9 | 181.9 | **1.42** (1.40–1.44) | 127.4 | 181.3 | 1.42 | 0.71 |
| centroid | poly1000 | int | 10,000 | 142.2 | 194.3 | **1.37** (1.35–1.38) | 141.6 | 193.7 | 1.37 | 0.63 |
| centroid | poly1000 | wkb | 10,000 | 69.3 | 69.5 | **1.00** (1.00–1.01) | 68.7 | 69.0 | 1.00 |  |
| intersects | points | sep | 1,000,000 | 434.9 | 473.2 | **1.09** (1.07–1.10) | 433.8 | 472.0 | 1.09 | 1.05 |
| intersects | points | int | 1,000,000 | 437.2 | 473.5 | **1.08** (1.07–1.09) | 436.0 | 472.4 | 1.08 | 1.06 |
| intersects | points | wkb | 1,000,000 | 448.1 | 457.1 | **1.02** (1.01–1.03) | 446.9 | 455.9 | 1.02 |  |
| intersects | poly10 | sep | 100,000 | 150.9 | 161.5 | **1.07** (1.06–1.09) | 150.0 | 160.5 | 1.07 | 0.98 |
| intersects | poly10 | int | 100,000 | 151.2 | 163.1 | **1.08** (1.07–1.09) | 150.2 | 162.1 | 1.08 | 0.97 |
| intersects | poly10 | wkb | 100,000 | 144.9 | 145.2 | **1.00** (0.99–1.01) | 143.9 | 144.2 | 1.00 |  |
| intersects | poly100 | sep | 20,000 | 210.2 | 221.7 | **1.05** (1.05–1.06) | 209.3 | 220.8 | 1.05 | 0.96 |
| intersects | poly100 | int | 20,000 | 216.1 | 225.1 | **1.04** (1.04–1.05) | 215.1 | 224.2 | 1.04 | 0.94 |
| intersects | poly100 | wkb | 20,000 | 199.9 | 199.8 | **1.00** (0.99–1.01) | 199.0 | 198.9 | 1.00 |  |
| intersects | poly1000 | sep | 2,000 | 428.8 | 437.8 | **1.02** (1.02–1.03) | 427.9 | 436.9 | 1.02 | 0.98 |
| intersects | poly1000 | int | 2,000 | 433.0 | 443.4 | **1.02** (1.02–1.03) | 432.1 | 442.4 | 1.02 | 0.97 |
| intersects | poly1000 | wkb | 2,000 | 416.4 | 416.8 | **1.00** (1.00–1.01) | 415.5 | 415.9 | 1.00 |  |
| buffer | points | sep | 100,000 | 462.8 | 468.4 | **1.01** (0.99–1.02) | 461.3 | 466.9 | 1.01 | 1.01 |
| buffer | points | int | 100,000 | 466.8 | 474.6 | **1.02** (1.01–1.04) | 465.3 | 473.1 | 1.02 | 1.01 |
| buffer | points | wkb | 100,000 | 483.5 | 466.4 | **0.98** (0.93–0.98) | 482.0 | 464.8 | 0.98 |  |
| buffer | poly10 | sep | 10,000 | 76.1 | 76.6 | **1.02** (1.00–1.06) | 75.1 | 75.6 | 1.02 | 0.99 |
| buffer | poly10 | int | 10,000 | 77.4 | 78.1 | **1.01** (0.99–1.10) | 76.4 | 77.0 | 1.01 | 0.98 |
| buffer | poly10 | wkb | 10,000 | 74.4 | 76.1 | **1.02** (1.01–1.04) | 73.4 | 75.1 | 1.02 |  |
| buffer | poly100 | sep | 2,000 | 458.4 | 456.4 | **1.00** (0.99–1.00) | 457.4 | 455.4 | 1.00 | 0.99 |
| buffer | poly100 | int | 2,000 | 459.5 | 455.4 | **0.99** (0.98–1.00) | 458.6 | 454.4 | 0.99 | 1.00 |
| buffer | poly100 | wkb | 2,000 | 458.6 | 455.7 | **0.99** (0.99–1.00) | 457.6 | 454.1 | 0.99 |  |
| buffer | poly1000 | sep | 200 | 5308.6 | 5310.4 | **1.00** (1.00–1.01) | 5307.6 | 5309.4 | 1.00 | 1.00 |
| buffer | poly1000 | int | 200 | 5335.6 | 5422.9 | **1.02** (1.01–1.03) | 5334.6 | 5421.9 | 1.02 | 1.02 |
| buffer | poly1000 | wkb | 200 | 5407.7 | 5582.4 | **1.03** (1.02–1.05) | 5406.7 | 5581.5 | 1.03 |  |

#### H2: instruction counts per row, end to end (cachegrind)

Ratio = WKB-output / native-output. Above 1.25 means native is ≥ 20% faster (time reduction of 20%).

| pipeline | input | enc | rows | unified native | unified WKB | **unified W/N** | typed native | typed WKB | typed W/N | fast-WKB unified W/N |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 100,000 | 3,075 | 2,674 | **0.87** | 849 | 1,471 | 1.73 | 1.01 |
| p1 | points | int | 100,000 | 3,088 | 2,700 | **0.87** | 907 | 1,521 | 1.68 | 1.01 |
| p1 | points | wkb | 100,000 | 2,464 | 2,041 | **0.83** | 1,340 | 1,961 | 1.46 | 1.01 |
| p1 | poly10 | sep | 10,000 | 9,168 | 8,760 | **0.96** | 4,492 | 5,120 | 1.14 | 1.01 |
| p1 | poly10 | int | 10,000 | 9,392 | 8,983 | **0.96** | 4,864 | 5,492 | 1.13 | 1.01 |
| p1 | poly10 | wkb | 10,000 | 5,669 | 5,258 | **0.93** | 4,549 | 5,178 | 1.14 | 1.01 |
| p1 | poly100 | sep | 10,000 | 44,549 | 44,103 | **0.99** | 25,488 | 26,056 | 1.02 | 1.00 |
| p1 | poly100 | int | 10,000 | 46,405 | 45,981 | **0.99** | 28,896 | 29,371 | 1.02 | 1.00 |
| p1 | poly100 | wkb | 10,000 | 22,489 | 22,135 | **0.98** | 21,349 | 22,055 | 1.03 | 1.01 |
| p1 | poly1000 | sep | 1,000 | 393,838 | 392,972 | **1.00** | 230,769 | 231,030 | 1.00 | 1.00 |
| p1 | poly1000 | int | 1,000 | 413,854 | 412,983 | **1.00** | 264,795 | 265,053 | 1.00 | 1.00 |
| p1 | poly1000 | wkb | 1,000 | 186,372 | 185,482 | **1.00** | 185,231 | 185,400 | 1.00 | 1.00 |
| p2 | points | sep | 100,000 | 4,913 | 3,135 | **0.64** | 2,549 | 1,834 | 0.72 | 0.60 |
| p2 | points | int | 100,000 | 4,932 | 3,149 | **0.64** | 2,605 | 1,895 | 0.73 | 0.59 |
| p2 | points | wkb | 100,000 | 4,285 | 2,505 | **0.58** | 3,049 | 2,337 | 0.77 | 0.58 |
| p2 | poly10 | sep | 10,000 | 32,982 | 27,214 | **0.83** | 26,078 | 23,287 | 0.89 | 0.79 |
| p2 | poly10 | int | 10,000 | 33,081 | 27,192 | **0.82** | 26,260 | 23,703 | 0.90 | 0.81 |
| p2 | poly10 | wkb | 10,000 | 29,324 | 23,489 | **0.80** | 26,029 | 23,269 | 0.89 | 0.80 |
| p2 | poly100 | sep | 10,000 | 258,979 | 244,316 | **0.94** | 232,090 | 225,696 | 0.97 | 0.94 |
| p2 | poly100 | int | 10,000 | 261,523 | 246,077 | **0.94** | 235,352 | 228,949 | 0.97 | 0.94 |
| p2 | poly100 | wkb | 10,000 | 236,379 | 222,144 | **0.94** | 228,064 | 220,871 | 0.97 | 0.94 |
| p2 | poly1000 | sep | 1,000 | 3,145,118 | 3,094,013 | **0.98** | 2,960,545 | 2,931,761 | 0.99 | 0.98 |
| p2 | poly1000 | int | 1,000 | 3,162,797 | 3,115,666 | **0.99** | 2,994,190 | 2,967,222 | 0.99 | 0.98 |
| p2 | poly1000 | wkb | 1,000 | 2,937,748 | 2,886,826 | **0.98** | 2,914,916 | 2,885,443 | 0.99 | 0.98 |
| p3 | points | sep | 100,000 | 8,894 | 7,128 | **0.80** | 6,548 | 5,901 | 0.90 | 0.79 |
| p3 | points | int | 100,000 | 8,922 | 7,148 | **0.80** | 6,599 | 5,934 | 0.90 | 0.79 |
| p3 | points | wkb | 100,000 | 8,273 | 6,475 | **0.78** | 7,049 | 6,403 | 0.91 | 0.78 |
| p3 | poly10 | sep | 10,000 | 63,549 | 57,138 | **0.90** | 56,579 | 53,366 | 0.94 | 0.89 |
| p3 | poly10 | int | 10,000 | 63,799 | 57,361 | **0.90** | 56,890 | 53,738 | 0.94 | 0.89 |
| p3 | poly10 | wkb | 10,000 | 60,055 | 53,526 | **0.89** | 56,625 | 53,453 | 0.94 | 0.89 |
| p3 | poly100 | sep | 10,000 | 507,735 | 476,182 | **0.94** | 473,984 | 458,170 | 0.97 | 0.94 |
| p3 | poly100 | int | 10,000 | 509,924 | 478,264 | **0.94** | 477,391 | 461,645 | 0.97 | 0.94 |
| p3 | poly100 | wkb | 10,000 | 485,998 | 454,160 | **0.93** | 469,988 | 454,088 | 0.97 | 0.93 |
| p3 | poly1000 | sep | 1,000 | 4,953,842 | 4,657,648 | **0.94** | 4,653,095 | 4,494,695 | 0.97 | 0.94 |
| p3 | poly1000 | int | 1,000 | 4,973,749 | 4,677,661 | **0.94** | 4,687,163 | 4,528,711 | 0.97 | 0.94 |
| p3 | poly1000 | wkb | 1,000 | 4,745,841 | 4,449,228 | **0.94** | 4,607,601 | 4,449,086 | 0.97 | 0.94 |
| p4 | points | sep | 10,000 | 249,759 | 230,995 | **0.92** | 240,128 | 228,442 | 0.95 | 0.92 |
| p4 | points | int | 10,000 | 249,365 | 231,098 | **0.93** | 239,993 | 228,231 | 0.95 | 0.93 |
| p4 | points | wkb | 10,000 | 249,096 | 230,389 | **0.92** | 240,551 | 228,743 | 0.95 | 0.92 |
| p4 | poly10 | sep | 1,000 | 393,697 | 357,330 | **0.91** | 377,665 | 352,120 | 0.93 | 0.91 |
| p4 | poly10 | int | 1,000 | 393,250 | 358,831 | **0.91** | 377,680 | 351,854 | 0.93 | 0.91 |
| p4 | poly10 | wkb | 1,000 | 389,150 | 354,363 | **0.91** | 377,548 | 352,812 | 0.93 | 0.91 |
| p4 | poly100 | sep | 200 | 6,322,159 | 6,088,150 | **0.96** | 6,239,321 | 6,070,876 | 0.97 | 0.96 |
| p4 | poly100 | int | 200 | 6,325,601 | 6,090,144 | **0.96** | 6,243,685 | 6,075,152 | 0.97 | 0.96 |
| p4 | poly100 | wkb | 200 | 6,294,637 | 6,062,666 | **0.96** | 6,236,743 | 6,063,885 | 0.97 | 0.96 |
| p4 | poly1000 | sep | 50 | 496,148,267 | 495,682,655 | **1.00** | 495,921,735 | 495,546,401 | 1.00 | 1.00 |
| p4 | poly1000 | int | 50 | 496,151,842 | 495,712,508 | **1.00** | 495,971,168 | 495,579,733 | 1.00 | 1.00 |
| p4 | poly1000 | wkb | 50 | 495,932,431 | 495,477,946 | **1.00** | 495,874,066 | 495,474,908 | 1.00 | 1.00 |

#### H2: wall clock (wall1)

| pipeline | input | enc | rows | unified native ms | unified WKB ms | **unified W/N** (min–max) | typed native ms | typed WKB ms | typed W/N (min–max) |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 1,000,000 | 114.3 | 112.4 | **0.98** (0.98–0.99) | 47.7 | 57.4 | 1.21 (1.20–1.21) |
| p1 | points | int | 1,000,000 | 116.3 | 112.8 | **0.97** (0.86–0.97) | 48.3 | 59.7 | 1.23 (1.22–1.29) |
| p1 | points | wkb | 1,000,000 | 97.1 | 82.4 | **0.85** (0.85–0.85) | 53.6 | 71.0 | 1.33 (1.32–1.33) |
| p1 | poly10 | sep | 100,000 | 37.4 | 36.4 | **0.97** (0.97–0.98) | 23.0 | 23.9 | 1.04 (1.03–1.05) |
| p1 | poly10 | int | 100,000 | 39.2 | 37.9 | **0.96** (0.95–0.97) | 24.3 | 25.2 | 1.04 (1.03–1.04) |
| p1 | poly10 | wkb | 100,000 | 22.0 | 21.0 | **0.95** (0.94–0.95) | 17.7 | 19.1 | 1.08 (1.05–1.10) |
| p1 | poly100 | sep | 100,000 | 198.4 | 197.4 | **1.00** (0.98–1.00) | 139.2 | 140.4 | 1.01 (1.01–1.01) |
| p1 | poly100 | int | 100,000 | 212.8 | 211.8 | **0.99** (0.98–1.00) | 154.0 | 154.7 | 1.01 (1.00–1.01) |
| p1 | poly100 | wkb | 100,000 | 84.9 | 83.5 | **0.98** (0.98–0.99) | 80.3 | 82.0 | 1.02 (1.02–1.02) |
| p1 | poly1000 | sep | 10,000 | 183.7 | 183.7 | **1.00** (1.00–1.00) | 129.3 | 129.4 | 1.00 (1.00–1.01) |
| p1 | poly1000 | int | 10,000 | 197.3 | 197.2 | **1.00** (0.99–1.01) | 142.9 | 142.9 | 1.00 (0.99–1.01) |
| p1 | poly1000 | wkb | 10,000 | 70.3 | 70.2 | **1.00** (0.99–1.00) | 69.9 | 69.9 | 1.00 (1.00–1.00) |
| p2 | points | sep | 1,000,000 | 173.1 | 114.6 | **0.66** (0.64–0.66) | 95.9 | 65.4 | 0.68 (0.68–0.69) |
| p2 | points | int | 1,000,000 | 175.6 | 119.0 | **0.68** (0.67–0.68) | 102.4 | 70.6 | 0.69 (0.68–0.69) |
| p2 | points | wkb | 1,000,000 | 153.1 | 96.2 | **0.63** (0.63–0.63) | 107.7 | 76.9 | 0.71 (0.71–0.72) |
| p2 | poly10 | sep | 100,000 | 121.6 | 102.1 | **0.84** (0.84–0.85) | 99.5 | 89.2 | 0.90 (0.88–0.90) |
| p2 | poly10 | int | 100,000 | 127.6 | 108.5 | **0.85** (0.84–0.85) | 102.2 | 92.3 | 0.90 (0.88–0.91) |
| p2 | poly10 | wkb | 100,000 | 119.0 | 86.9 | **0.73** (0.72–0.74) | 95.6 | 83.8 | 0.88 (0.87–0.89) |
| p2 | poly100 | sep | 100,000 | 1023.7 | 959.4 | **0.94** (0.94–0.94) | 937.2 | 905.3 | 0.97 (0.96–0.97) |
| p2 | poly100 | int | 100,000 | 1035.0 | 976.6 | **0.94** (0.94–0.95) | 967.6 | 931.9 | 0.96 (0.87–0.97) |
| p2 | poly100 | wkb | 100,000 | 906.6 | 848.1 | **0.94** (0.93–0.94) | 879.5 | 845.9 | 0.96 (0.96–0.97) |
| p2 | poly1000 | sep | 10,000 | 1148.3 | 1129.4 | **0.98** (0.98–0.99) | 1085.5 | 1073.2 | 0.99 (0.99–0.99) |
| p2 | poly1000 | int | 10,000 | 1158.9 | 1142.0 | **0.99** (0.98–0.99) | 1098.1 | 1090.0 | 0.99 (0.99–0.99) |
| p2 | poly1000 | wkb | 10,000 | 1032.3 | 1017.3 | **0.99** (0.98–0.99) | 1026.1 | 1017.3 | 0.99 (0.99–0.99) |
| p3 | points | sep | 1,000,000 | 306.2 | 248.6 | **0.81** (0.80–0.82) | 222.3 | 207.7 | 0.93 (0.91–0.96) |
| p3 | points | int | 1,000,000 | 300.3 | 244.5 | **0.81** (0.81–0.81) | 224.3 | 201.9 | 0.90 (0.90–0.92) |
| p3 | points | wkb | 1,000,000 | 288.4 | 226.6 | **0.79** (0.78–0.81) | 233.3 | 217.5 | 0.92 (0.91–0.94) |
| p3 | poly10 | sep | 100,000 | 211.2 | 191.2 | **0.91** (0.90–0.94) | 185.4 | 177.5 | 0.96 (0.95–0.97) |
| p3 | poly10 | int | 100,000 | 212.5 | 189.7 | **0.89** (0.89–0.90) | 186.6 | 178.1 | 0.95 (0.94–0.96) |
| p3 | poly10 | wkb | 100,000 | 195.3 | 171.7 | **0.88** (0.88–0.89) | 180.3 | 171.4 | 0.95 (0.94–0.96) |
| p3 | poly100 | sep | 100,000 | 1694.3 | 1541.7 | **0.91** (0.91–0.92) | 1536.7 | 1464.9 | 0.95 (0.95–0.96) |
| p3 | poly100 | int | 100,000 | 1716.5 | 1581.1 | **0.92** (0.91–0.93) | 1579.5 | 1500.0 | 0.95 (0.94–0.96) |
| p3 | poly100 | wkb | 100,000 | 1558.2 | 1410.2 | **0.90** (0.90–0.91) | 1474.4 | 1411.2 | 0.96 (0.95–0.96) |
| p3 | poly1000 | sep | 10,000 | 1578.2 | 1441.0 | **0.91** (0.91–0.92) | 1460.4 | 1388.9 | 0.95 (0.95–0.95) |
| p3 | poly1000 | int | 10,000 | 1606.4 | 1457.9 | **0.91** (0.90–0.91) | 1486.7 | 1414.8 | 0.95 (0.90–1.02) |
| p3 | poly1000 | wkb | 10,000 | 1468.1 | 1344.8 | **0.91** (0.90–0.92) | 1417.6 | 1347.5 | 0.95 (0.90–0.97) |
| p4 | points | sep | 100,000 | 781.4 | 686.3 | **0.88** (0.87–0.89) | 723.8 | 681.8 | 0.94 (0.93–0.94) |
| p4 | points | int | 100,000 | 776.8 | 699.8 | **0.89** (0.86–0.90) | 729.2 | 680.2 | 0.94 (0.93–0.94) |
| p4 | points | wkb | 100,000 | 786.3 | 693.3 | **0.88** (0.86–0.93) | 737.2 | 687.6 | 0.93 (0.88–0.96) |
| p4 | poly10 | sep | 10,000 | 123.1 | 111.9 | **0.90** (0.90–0.91) | 118.7 | 111.1 | 0.94 (0.92–0.95) |
| p4 | poly10 | int | 10,000 | 123.3 | 111.4 | **0.90** (0.89–0.92) | 116.8 | 109.3 | 0.94 (0.93–0.98) |
| p4 | poly10 | wkb | 10,000 | 122.0 | 110.7 | **0.90** (0.89–0.93) | 114.7 | 107.8 | 0.95 (0.93–0.95) |
| p4 | poly100 | sep | 2,000 | 528.0 | 512.5 | **0.97** (0.96–0.98) | 525.6 | 516.0 | 0.98 (0.97–0.98) |
| p4 | poly100 | int | 2,000 | 525.4 | 510.4 | **0.97** (0.96–0.97) | 525.5 | 515.8 | 0.98 (0.98–0.99) |
| p4 | poly100 | wkb | 2,000 | 523.9 | 508.1 | **0.97** (0.97–0.97) | 524.3 | 515.0 | 0.98 (0.98–0.99) |
| p4 | poly1000 | sep | 200 | 5356.5 | 5352.9 | **1.00** (1.00–1.01) | 5367.6 | 5357.1 | 1.00 (0.99–1.01) |
| p4 | poly1000 | int | 200 | 5347.6 | 5338.5 | **1.00** (0.99–1.01) | 5330.4 | 5327.9 | 1.00 (0.99–1.00) |
| p4 | poly1000 | wkb | 200 | 5414.5 | 5374.2 | **0.99** (0.98–1.00) | 5334.8 | 5334.4 | 1.00 (0.99–1.00) |

#### H2: wall clock (wall2)

| pipeline | input | enc | rows | unified native ms | unified WKB ms | **unified W/N** (min–max) | typed native ms | typed WKB ms | typed W/N (min–max) |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|
| p1 | points | sep | 1,000,000 | 114.9 | 111.8 | **0.97** (0.97–0.98) | 46.5 | 57.0 | 1.22 (1.22–1.23) |
| p1 | points | int | 1,000,000 | 116.5 | 112.5 | **0.97** (0.90–0.98) | 48.1 | 60.1 | 1.25 (1.24–1.26) |
| p1 | points | wkb | 1,000,000 | 96.9 | 82.1 | **0.85** (0.85–0.85) | 53.2 | 70.5 | 1.32 (1.32–1.33) |
| p1 | poly10 | sep | 100,000 | 37.5 | 36.2 | **0.97** (0.97–0.99) | 23.5 | 24.3 | 1.04 (1.03–1.04) |
| p1 | poly10 | int | 100,000 | 39.0 | 37.8 | **0.97** (0.97–0.97) | 24.0 | 25.1 | 1.04 (1.03–1.05) |
| p1 | poly10 | wkb | 100,000 | 21.7 | 20.7 | **0.96** (0.95–0.96) | 17.8 | 19.2 | 1.08 (1.07–1.09) |
| p1 | poly100 | sep | 100,000 | 196.8 | 195.3 | **0.99** (0.99–1.00) | 137.8 | 139.4 | 1.01 (1.01–1.02) |
| p1 | poly100 | int | 100,000 | 210.6 | 209.6 | **1.00** (0.99–1.00) | 153.4 | 153.9 | 1.00 (0.99–1.03) |
| p1 | poly100 | wkb | 100,000 | 84.3 | 83.1 | **0.99** (0.96–0.99) | 80.2 | 82.3 | 1.03 (1.02–1.03) |
| p1 | poly1000 | sep | 10,000 | 182.6 | 182.2 | **1.00** (1.00–1.01) | 128.4 | 128.4 | 1.00 (1.00–1.00) |
| p1 | poly1000 | int | 10,000 | 197.4 | 197.1 | **1.00** (1.00–1.02) | 141.5 | 142.2 | 1.00 (0.98–1.01) |
| p1 | poly1000 | wkb | 10,000 | 70.0 | 69.9 | **1.00** (1.00–1.00) | 69.5 | 69.6 | 1.00 (1.00–1.01) |
| p2 | points | sep | 1,000,000 | 173.9 | 117.1 | **0.67** (0.67–0.68) | 97.4 | 68.0 | 0.70 (0.69–0.70) |
| p2 | points | int | 1,000,000 | 179.4 | 122.9 | **0.68** (0.68–0.69) | 104.5 | 74.8 | 0.72 (0.71–0.72) |
| p2 | points | wkb | 1,000,000 | 156.4 | 99.6 | **0.64** (0.62–0.64) | 109.7 | 79.5 | 0.73 (0.71–0.73) |
| p2 | poly10 | sep | 100,000 | 122.5 | 104.1 | **0.85** (0.83–0.91) | 99.6 | 89.5 | 0.90 (0.89–0.91) |
| p2 | poly10 | int | 100,000 | 126.3 | 106.8 | **0.84** (0.84–0.86) | 105.2 | 94.3 | 0.90 (0.89–0.91) |
| p2 | poly10 | wkb | 100,000 | 118.9 | 87.5 | **0.74** (0.72–0.75) | 95.6 | 85.1 | 0.89 (0.88–0.90) |
| p2 | poly100 | sep | 100,000 | 1018.9 | 952.7 | **0.94** (0.93–0.94) | 934.1 | 904.2 | 0.97 (0.95–0.97) |
| p2 | poly100 | int | 100,000 | 1038.4 | 983.4 | **0.94** (0.93–0.96) | 949.5 | 916.8 | 0.97 (0.96–0.97) |
| p2 | poly100 | wkb | 100,000 | 905.5 | 846.3 | **0.93** (0.93–0.94) | 876.9 | 842.9 | 0.96 (0.96–0.97) |
| p2 | poly1000 | sep | 10,000 | 1144.8 | 1129.5 | **0.98** (0.98–0.99) | 1087.0 | 1075.1 | 0.99 (0.99–0.99) |
| p2 | poly1000 | int | 10,000 | 1155.8 | 1143.4 | **0.99** (0.99–0.99) | 1096.2 | 1086.0 | 0.99 (0.99–1.00) |
| p2 | poly1000 | wkb | 10,000 | 1030.7 | 1014.9 | **0.98** (0.98–0.99) | 1023.9 | 1014.6 | 0.99 (0.99–1.00) |
| p3 | points | sep | 1,000,000 | 311.1 | 249.1 | **0.80** (0.79–0.82) | 223.8 | 206.1 | 0.92 (0.91–0.93) |
| p3 | points | int | 1,000,000 | 312.9 | 255.3 | **0.81** (0.81–0.82) | 227.7 | 207.3 | 0.91 (0.90–0.92) |
| p3 | points | wkb | 1,000,000 | 286.2 | 227.0 | **0.79** (0.78–0.80) | 235.6 | 215.5 | 0.92 (0.91–0.92) |
| p3 | poly10 | sep | 100,000 | 212.0 | 189.0 | **0.89** (0.88–0.91) | 185.2 | 174.7 | 0.94 (0.94–0.95) |
| p3 | poly10 | int | 100,000 | 212.9 | 189.5 | **0.89** (0.86–0.89) | 187.1 | 177.1 | 0.95 (0.94–0.95) |
| p3 | poly10 | wkb | 100,000 | 195.0 | 169.4 | **0.87** (0.87–0.87) | 181.1 | 168.6 | 0.93 (0.92–0.94) |
| p3 | poly100 | sep | 100,000 | 1685.1 | 1545.5 | **0.92** (0.91–0.92) | 1580.4 | 1612.6 | 1.02 (1.02–1.02) |
| p3 | poly100 | int | 100,000 | 1713.4 | 1564.8 | **0.91** (0.91–0.92) | 1560.1 | 1491.4 | 0.96 (0.95–0.96) |
| p3 | poly100 | wkb | 100,000 | 1553.5 | 1416.6 | **0.91** (0.91–0.91) | 1495.1 | 1418.3 | 0.95 (0.95–0.96) |
| p3 | poly1000 | sep | 10,000 | 1585.7 | 1444.9 | **0.91** (0.91–0.92) | 1459.1 | 1392.9 | 0.96 (0.95–0.96) |
| p3 | poly1000 | int | 10,000 | 1607.7 | 1459.4 | **0.91** (0.90–0.91) | 1485.4 | 1404.4 | 0.95 (0.94–0.95) |
| p3 | poly1000 | wkb | 10,000 | 1467.1 | 1336.0 | **0.91** (0.87–0.91) | 1399.8 | 1333.2 | 0.95 (0.95–0.96) |
| p4 | points | sep | 100,000 | 780.7 | 691.8 | **0.89** (0.87–0.89) | 735.6 | 682.2 | 0.93 (0.93–0.94) |
| p4 | points | int | 100,000 | 774.0 | 683.2 | **0.88** (0.88–0.89) | 725.7 | 682.1 | 0.94 (0.92–0.95) |
| p4 | points | wkb | 100,000 | 779.7 | 680.4 | **0.87** (0.83–0.89) | 729.7 | 681.2 | 0.93 (0.89–0.95) |
| p4 | poly10 | sep | 10,000 | 121.8 | 110.0 | **0.90** (0.89–0.92) | 116.2 | 109.5 | 0.94 (0.92–0.96) |
| p4 | poly10 | int | 10,000 | 121.6 | 110.1 | **0.91** (0.90–0.93) | 115.8 | 108.7 | 0.93 (0.93–0.94) |
| p4 | poly10 | wkb | 10,000 | 119.7 | 107.9 | **0.90** (0.90–0.92) | 113.5 | 106.7 | 0.94 (0.92–0.94) |
| p4 | poly100 | sep | 2,000 | 524.7 | 508.1 | **0.97** (0.97–0.97) | 524.3 | 514.2 | 0.98 (0.98–0.99) |
| p4 | poly100 | int | 2,000 | 525.0 | 508.3 | **0.97** (0.97–0.98) | 523.1 | 513.5 | 0.98 (0.97–0.98) |
| p4 | poly100 | wkb | 2,000 | 521.5 | 507.3 | **0.97** (0.97–0.98) | 523.9 | 514.2 | 0.98 (0.98–0.99) |
| p4 | poly1000 | sep | 200 | 5345.8 | 5353.4 | **1.00** (0.97–1.00) | 5301.4 | 5287.2 | 1.00 (0.99–1.05) |
| p4 | poly1000 | int | 200 | 5373.8 | 5357.0 | **1.00** (0.99–1.01) | 5309.7 | 5323.3 | 1.00 (1.00–1.00) |
| p4 | poly1000 | wkb | 200 | 5369.3 | 5359.5 | **1.00** (0.98–1.00) | 5330.3 | 5318.7 | 1.00 (0.99–1.01) |

#### H6: instruction counts per row, end to end (cachegrind)

`expr-geo` = geodatafusion's current UDF (calls geoarrow-expr-geo). Ratio = owned / expr-geo; ≤ 1.10 is within 10%.

| function | input | enc | rows | expr-geo | owned | **owned / expr-geo** | owned (same type) | same / expr-geo |
|---|---|---|--:|--:|--:|--:|--:|--:|
| area | points | sep | 100,000 | 47.9 | 1,651 | **34.51** |  |  |
| area | points | int | 100,000 | 46.6 | 1,675 | **35.92** |  |  |
| area | points | wkb | 100,000 | 1,116 | 1,051 | **0.94** |  |  |
| area | poly10 | sep | 10,000 | 3,376 | 7,058 | **2.09** |  |  |
| area | poly10 | int | 10,000 | 3,753 | 7,281 | **1.94** |  |  |
| area | poly10 | wkb | 10,000 | 3,490 | 3,557 | **1.02** |  |  |
| area | poly100 | sep | 10,000 | 20,178 | 38,297 | **1.90** |  |  |
| area | poly100 | int | 10,000 | 23,617 | 40,369 | **1.71** |  |  |
| area | poly100 | wkb | 10,000 | 16,095 | 16,252 | **1.01** |  |  |
| area | poly1000 | sep | 1,000 | 184,242 | 345,708 | **1.88** |  |  |
| area | poly1000 | int | 1,000 | 218,326 | 365,718 | **1.68** |  |  |
| area | poly1000 | wkb | 1,000 | 138,974 | 137,905 | **0.99** |  |  |
| centroid | points | sep | 100,000 | 813 | 1,808 | **2.22** |  |  |
| centroid | points | int | 100,000 | 851 | 1,833 | **2.16** |  |  |
| centroid | points | wkb | 100,000 | 1,248 | 1,205 | **0.96** |  |  |
| centroid | poly10 | sep | 10,000 | 4,132 | 7,890 | **1.91** |  |  |
| centroid | poly10 | int | 10,000 | 4,508 | 8,112 | **1.80** |  |  |
| centroid | poly10 | wkb | 10,000 | 4,264 | 4,386 | **1.03** |  |  |
| centroid | poly100 | sep | 10,000 | 25,080 | 43,294 | **1.73** |  |  |
| centroid | poly100 | int | 10,000 | 28,517 | 45,315 | **1.59** |  |  |
| centroid | poly100 | wkb | 10,000 | 20,920 | 21,219 | **1.01** |  |  |
| centroid | poly1000 | sep | 1,000 | 230,996 | 392,274 | **1.70** |  |  |
| centroid | poly1000 | int | 1,000 | 265,066 | 412,289 | **1.56** |  |  |
| centroid | poly1000 | wkb | 1,000 | 185,563 | 184,797 | **1.00** |  |  |
| simplify | points | sep | 100,000 | 30.7 | 2,770 | **90.19** | 2,770 | 90.18 |
| simplify | points | int | 100,000 | 29.3 | 2,798 | **95.36** | 2,798 | 95.36 |
| simplify | points | wkb | 100,000 | 2,143 | 2,177 | **1.02** | 2,176 | 1.02 |
| simplify | poly10 | sep | 10,000 | 20,534 | 26,694 | **1.30** | 24,508 | 1.19 |
| simplify | poly10 | int | 10,000 | 20,736 | 26,800 | **1.29** | 24,679 | 1.19 |
| simplify | poly10 | wkb | 10,000 | 22,890 | 22,844 | **1.00** | 23,423 | 1.02 |
| simplify | poly100 | sep | 10,000 | 220,631 | 241,741 | **1.10** | 239,297 | 1.08 |
| simplify | poly100 | int | 10,000 | 223,997 | 243,931 | **1.09** | 241,186 | 1.08 |
| simplify | poly100 | wkb | 10,000 | 219,280 | 219,820 | **1.00** | 220,212 | 1.00 |
| simplify | poly1000 | sep | 1,000 | 2,921,518 | 3,097,879 | **1.06** | 3,087,400 | 1.06 |
| simplify | poly1000 | int | 1,000 | 2,955,028 | 3,117,623 | **1.06** | 3,102,818 | 1.05 |
| simplify | poly1000 | wkb | 1,000 | 2,891,059 | 2,891,666 | **1.00** | 2,891,407 | 1.00 |
| intersects (relate) | points | sep | 10,000 | 234,469 | 250,671 | **1.07** |  |  |
| intersects (relate) | points | int | 10,000 | 239,043 | 253,434 | **1.06** |  |  |
| intersects (relate) | points | wkb | 10,000 | 231,265 | 229,366 | **0.99** |  |  |
| intersects (relate) | poly10 | sep | 10,000 | 265,992 | 282,110 | **1.06** |  |  |
| intersects (relate) | poly10 | int | 10,000 | 269,574 | 286,026 | **1.06** |  |  |
| intersects (relate) | poly10 | wkb | 10,000 | 260,424 | 256,678 | **0.99** |  |  |
| intersects (relate) | poly100 | sep | 2,000 | 482,531 | 512,202 | **1.06** |  |  |
| intersects (relate) | poly100 | int | 2,000 | 490,592 | 516,355 | **1.05** |  |  |
| intersects (relate) | poly100 | wkb | 2,000 | 474,532 | 468,763 | **0.99** |  |  |
| intersects (relate) | poly1000 | sep | 500 | 4,283,645 | 4,428,521 | **1.03** |  |  |
| intersects (relate) | poly1000 | int | 500 | 4,320,990 | 4,451,443 | **1.03** |  |  |
| intersects (relate) | poly1000 | wkb | 500 | 4,231,889 | 4,200,535 | **0.99** |  |  |
| intersects (trait) | points | sep | 10,000 | 21,340 | 40,556 | **1.90** |  |  |
| intersects (trait) | points | int | 10,000 | 24,711 | 42,481 | **1.72** |  |  |
| intersects (trait) | points | wkb | 10,000 | 17,711 | 17,777 | **1.00** |  |  |
| intersects (trait) | poly10 | sep | 10,000 | 58,887 | 80,702 | **1.37** |  |  |
| intersects (trait) | poly10 | int | 10,000 | 62,754 | 82,996 | **1.32** |  |  |
| intersects (trait) | poly10 | wkb | 10,000 | 55,013 | 55,192 | **1.00** |  |  |
| intersects (trait) | poly100 | sep | 2,000 | 355,886 | 391,918 | **1.10** |  |  |
| intersects (trait) | poly100 | int | 2,000 | 362,710 | 395,925 | **1.09** |  |  |
| intersects (trait) | poly100 | wkb | 2,000 | 347,785 | 347,888 | **1.00** |  |  |
| intersects (trait) | poly1000 | sep | 500 | 3,031,201 | 3,208,152 | **1.06** |  |  |
| intersects (trait) | poly1000 | int | 500 | 3,068,566 | 3,230,122 | **1.05** |  |  |
| intersects (trait) | poly1000 | wkb | 500 | 2,981,377 | 2,978,270 | **1.00** |  |  |
| intersects_const | points | sep | 100,000 | 11,763 | 12,813 | **1.09** |  |  |
| intersects_const | points | int | 100,000 | 11,859 | 12,815 | **1.08** |  |  |
| intersects_const | points | wkb | 100,000 | 12,133 | 12,148 | **1.00** |  |  |
| intersects_const | poly10 | sep | 10,000 | 43,212 | 46,694 | **1.08** |  |  |
| intersects_const | poly10 | int | 10,000 | 43,474 | 46,994 | **1.08** |  |  |
| intersects_const | poly10 | wkb | 10,000 | 43,156 | 43,148 | **1.00** |  |  |
| intersects_const | poly100 | sep | 2,000 | 265,229 | 279,771 | **1.05** |  |  |
| intersects_const | poly100 | int | 2,000 | 269,007 | 282,071 | **1.05** |  |  |
| intersects_const | poly100 | wkb | 2,000 | 261,060 | 258,288 | **0.99** |  |  |
| intersects_const | poly1000 | sep | 500 | 4,089,144 | 4,220,750 | **1.03** |  |  |
| intersects_const | poly1000 | int | 500 | 4,121,785 | 4,239,275 | **1.03** |  |  |
| intersects_const | poly1000 | wkb | 500 | 4,040,779 | 4,011,800 | **0.99** |  |  |

#### H6: typed owned kernels vs expr-geo (cachegrind, e2e per row)

The H1 typed variants (`t_area`, `t_centroid`: owned kernel, `downcast_geoarrow_array!` loop, same rows) against today's expr-geo UDFs.

| function | input | enc | rows | expr-geo | owned typed | **ratio** |
|---|---|---|--:|--:|--:|--:|
| area | points | sep | 100,000 | 47.9 | 476 | **9.94** |
| area | points | int | 100,000 | 46.6 | 533 | **11.44** |
| area | points | wkb | 100,000 | 1,116 | 961 | **0.86** |
| area | poly10 | sep | 10,000 | 3,376 | 3,409 | **1.01** |
| area | poly10 | int | 10,000 | 3,753 | 3,782 | **1.01** |
| area | poly10 | wkb | 10,000 | 3,490 | 3,466 | **0.99** |
| area | poly100 | sep | 10,000 | 20,178 | 20,295 | **1.01** |
| area | poly100 | int | 10,000 | 23,617 | 23,693 | **1.00** |
| area | poly100 | wkb | 10,000 | 16,095 | 16,148 | **1.00** |
| area | poly1000 | sep | 1,000 | 184,242 | 183,358 | **1.00** |
| area | poly1000 | int | 1,000 | 218,326 | 217,378 | **1.00** |
| area | poly1000 | wkb | 1,000 | 138,974 | 137,980 | **0.99** |
| centroid | points | sep | 100,000 | 813 | 627 | **0.77** |
| centroid | points | int | 100,000 | 851 | 685 | **0.81** |
| centroid | points | wkb | 100,000 | 1,248 | 1,118 | **0.90** |
| centroid | poly10 | sep | 10,000 | 4,132 | 4,242 | **1.03** |
| centroid | poly10 | int | 10,000 | 4,508 | 4,615 | **1.02** |
| centroid | poly10 | wkb | 10,000 | 4,264 | 4,300 | **1.01** |
| centroid | poly100 | sep | 10,000 | 25,080 | 25,238 | **1.01** |
| centroid | poly100 | int | 10,000 | 28,517 | 28,696 | **1.01** |
| centroid | poly100 | wkb | 10,000 | 20,920 | 21,133 | **1.01** |
| centroid | poly1000 | sep | 1,000 | 230,996 | 230,253 | **1.00** |
| centroid | poly1000 | int | 1,000 | 265,066 | 264,276 | **1.00** |
| centroid | poly1000 | wkb | 1,000 | 185,563 | 184,711 | **1.00** |

#### H6: wall clock (wall1)

| function | input | enc | rows | expr-geo ms | owned ms | **owned / expr-geo** (min–max) | same / expr-geo |
|---|---|---|--:|--:|--:|--:|--:|
| area | points | sep | 1,000,000 | 0.8 | 60.4 | **72.45** (30.78–119.88) |  |
| area | points | int | 1,000,000 | 2.0 | 69.3 | **35.07** (29.69–78.12) |  |
| area | points | wkb | 1,000,000 | 36.0 | 42.2 | **1.17** (1.17–1.20) |  |
| area | poly10 | sep | 100,000 | 17.8 | 29.8 | **1.68** (1.61–1.68) |  |
| area | poly10 | int | 100,000 | 19.4 | 31.5 | **1.63** (1.63–1.64) |  |
| area | poly10 | wkb | 100,000 | 12.9 | 13.7 | **1.07** (1.06–1.07) |  |
| area | poly100 | sep | 100,000 | 124.5 | 180.7 | **1.45** (1.45–1.47) |  |
| area | poly100 | int | 100,000 | 138.2 | 195.0 | **1.42** (1.41–1.44) |  |
| area | poly100 | wkb | 100,000 | 66.2 | 66.9 | **1.01** (1.00–1.03) |  |
| area | poly1000 | sep | 10,000 | 118.4 | 172.5 | **1.46** (1.45–1.46) |  |
| area | poly1000 | int | 10,000 | 131.7 | 185.2 | **1.41** (1.39–1.42) |  |
| area | poly1000 | wkb | 10,000 | 59.1 | 59.2 | **1.00** (1.00–1.01) |  |
| centroid | points | sep | 1,000,000 | 33.8 | 68.2 | **2.02** (2.01–2.03) |  |
| centroid | points | int | 1,000,000 | 34.0 | 71.5 | **2.10** (2.10–2.11) |  |
| centroid | points | wkb | 1,000,000 | 40.9 | 49.6 | **1.21** (1.18–1.23) |  |
| centroid | poly10 | sep | 100,000 | 20.3 | 33.0 | **1.62** (1.61–1.63) |  |
| centroid | poly10 | int | 100,000 | 21.9 | 34.7 | **1.58** (1.56–1.59) |  |
| centroid | poly10 | wkb | 100,000 | 16.0 | 16.8 | **1.06** (1.04–1.06) |  |
| centroid | poly100 | sep | 100,000 | 136.6 | 192.9 | **1.41** (1.41–1.42) |  |
| centroid | poly100 | int | 100,000 | 150.4 | 206.2 | **1.37** (1.36–1.39) |  |
| centroid | poly100 | wkb | 100,000 | 78.3 | 79.8 | **1.02** (0.99–1.04) |  |
| centroid | poly1000 | sep | 10,000 | 127.9 | 181.6 | **1.42** (1.42–1.42) |  |
| centroid | poly1000 | int | 10,000 | 142.7 | 196.0 | **1.37** (1.33–1.38) |  |
| centroid | poly1000 | wkb | 10,000 | 69.7 | 69.7 | **1.00** (0.99–1.00) |  |
| simplify | points | sep | 1,000,000 | 0.8 | 92.7 | **115.60** (112.55–117.82) | 114.30 |
| simplify | points | int | 1,000,000 | 0.8 | 94.7 | **123.24** (112.33–126.96) | 122.90 |
| simplify | points | wkb | 1,000,000 | 67.4 | 74.8 | **1.11** (1.09–1.12) | 1.11 |
| simplify | poly10 | sep | 100,000 | 80.0 | 96.8 | **1.21** (1.20–1.22) | 1.20 |
| simplify | poly10 | int | 100,000 | 82.3 | 99.5 | **1.21** (1.20–1.23) | 1.20 |
| simplify | poly10 | wkb | 100,000 | 82.4 | 85.4 | **1.04** (1.03–1.04) | 1.03 |
| simplify | poly100 | sep | 100,000 | 894.4 | 941.5 | **1.06** (1.04–1.07) | 1.07 |
| simplify | poly100 | int | 100,000 | 906.0 | 954.4 | **1.05** (1.04–1.07) | 1.07 |
| simplify | poly100 | wkb | 100,000 | 843.8 | 841.0 | **1.00** (0.99–1.00) | 1.00 |
| simplify | poly1000 | sep | 10,000 | 1065.0 | 1122.4 | **1.05** (1.05–1.06) | 1.05 |
| simplify | poly1000 | int | 10,000 | 1087.5 | 1138.8 | **1.05** (1.04–1.05) | 1.04 |
| simplify | poly1000 | wkb | 10,000 | 1009.1 | 1010.3 | **1.00** (1.00–1.00) | 1.00 |
| intersects (relate) | points | sep | 100,000 | 725.1 | 782.9 | **1.08** (1.07–1.09) |  |
| intersects (relate) | points | int | 100,000 | 737.9 | 793.8 | **1.08** (1.06–1.08) |  |
| intersects (relate) | points | wkb | 100,000 | 678.5 | 677.4 | **1.00** (0.99–1.01) |  |
| intersects (relate) | poly10 | sep | 100,000 | 848.3 | 906.7 | **1.08** (1.05–1.08) |  |
| intersects (relate) | poly10 | int | 100,000 | 865.8 | 928.3 | **1.07** (1.05–1.08) |  |
| intersects (relate) | poly10 | wkb | 100,000 | 785.9 | 780.5 | **0.99** (0.99–1.00) |  |
| intersects (relate) | poly100 | sep | 20,000 | 355.2 | 379.4 | **1.07** (1.06–1.09) |  |
| intersects (relate) | poly100 | int | 20,000 | 362.5 | 384.9 | **1.07** (1.03–1.07) |  |
| intersects (relate) | poly100 | wkb | 20,000 | 333.7 | 328.0 | **0.99** (0.97–0.99) |  |
| intersects (relate) | poly1000 | sep | 2,000 | 454.5 | 463.1 | **1.02** (1.01–1.02) |  |
| intersects (relate) | poly1000 | int | 2,000 | 455.8 | 466.5 | **1.02** (1.02–1.03) |  |
| intersects (relate) | poly1000 | wkb | 2,000 | 450.7 | 439.2 | **0.97** (0.97–0.98) |  |
| intersects (trait) | points | sep | 100,000 | 129.8 | 188.8 | **1.46** (1.45–1.46) |  |
| intersects (trait) | points | int | 100,000 | 142.7 | 201.1 | **1.41** (1.40–1.41) |  |
| intersects (trait) | points | wkb | 100,000 | 72.0 | 73.3 | **1.02** (0.99–1.02) |  |
| intersects (trait) | poly10 | sep | 100,000 | 222.2 | 290.4 | **1.31** (1.29–1.31) |  |
| intersects (trait) | poly10 | int | 100,000 | 237.6 | 307.0 | **1.29** (1.29–1.30) |  |
| intersects (trait) | poly10 | wkb | 100,000 | 157.9 | 161.7 | **1.02** (1.01–1.05) |  |
| intersects (trait) | poly100 | sep | 20,000 | 181.7 | 210.8 | **1.16** (1.15–1.18) |  |
| intersects (trait) | poly100 | int | 20,000 | 187.0 | 217.1 | **1.16** (1.14–1.16) |  |
| intersects (trait) | poly100 | wkb | 20,000 | 158.4 | 159.8 | **1.01** (1.00–1.04) |  |
| intersects (trait) | poly1000 | sep | 2,000 | 152.7 | 164.4 | **1.07** (1.07–1.10) |  |
| intersects (trait) | poly1000 | int | 2,000 | 156.3 | 167.3 | **1.07** (1.06–1.08) |  |
| intersects (trait) | poly1000 | wkb | 2,000 | 140.5 | 140.5 | **1.00** (1.00–1.00) |  |
| intersects_const | points | sep | 1,000,000 | 411.0 | 479.1 | **1.17** (1.16–1.18) |  |
| intersects_const | points | int | 1,000,000 | 412.6 | 491.0 | **1.19** (1.19–1.20) |  |
| intersects_const | points | wkb | 1,000,000 | 431.7 | 458.6 | **1.06** (1.04–1.08) |  |
| intersects_const | poly10 | sep | 100,000 | 148.1 | 161.5 | **1.09** (1.09–1.09) |  |
| intersects_const | poly10 | int | 100,000 | 150.3 | 163.7 | **1.09** (1.08–1.09) |  |
| intersects_const | poly10 | wkb | 100,000 | 145.3 | 144.7 | **1.00** (0.98–1.01) |  |
| intersects_const | poly100 | sep | 20,000 | 213.2 | 222.4 | **1.04** (1.04–1.05) |  |
| intersects_const | poly100 | int | 20,000 | 215.3 | 224.5 | **1.05** (1.04–1.06) |  |
| intersects_const | poly100 | wkb | 20,000 | 199.9 | 200.6 | **1.00** (0.99–1.01) |  |
| intersects_const | poly1000 | sep | 2,000 | 430.9 | 438.2 | **1.02** (1.01–1.02) |  |
| intersects_const | poly1000 | int | 2,000 | 436.1 | 443.9 | **1.02** (1.01–1.02) |  |
| intersects_const | poly1000 | wkb | 2,000 | 418.7 | 416.9 | **1.00** (1.00–1.00) |  |

#### H6: wall clock (wall2)

| function | input | enc | rows | expr-geo ms | owned ms | **owned / expr-geo** (min–max) | same / expr-geo |
|---|---|---|--:|--:|--:|--:|--:|
| area | points | sep | 1,000,000 | 0.9 | 60.2 | **71.06** (67.67–117.44) |  |
| area | points | int | 1,000,000 | 1.9 | 69.7 | **33.63** (30.68–71.05) |  |
| area | points | wkb | 1,000,000 | 35.7 | 42.5 | **1.19** (1.19–1.20) |  |
| area | poly10 | sep | 100,000 | 18.0 | 30.1 | **1.67** (1.65–1.68) |  |
| area | poly10 | int | 100,000 | 19.2 | 31.6 | **1.64** (1.63–1.70) |  |
| area | poly10 | wkb | 100,000 | 12.9 | 13.7 | **1.06** (1.04–1.07) |  |
| area | poly100 | sep | 100,000 | 123.7 | 179.0 | **1.45** (1.44–1.45) |  |
| area | poly100 | int | 100,000 | 138.5 | 194.3 | **1.41** (1.27–1.41) |  |
| area | poly100 | wkb | 100,000 | 66.1 | 67.5 | **1.02** (1.01–1.03) |  |
| area | poly1000 | sep | 10,000 | 117.8 | 172.1 | **1.46** (1.46–1.47) |  |
| area | poly1000 | int | 10,000 | 131.5 | 184.7 | **1.41** (1.39–1.41) |  |
| area | poly1000 | wkb | 10,000 | 58.9 | 59.0 | **1.00** (1.00–1.01) |  |
| centroid | points | sep | 1,000,000 | 32.4 | 68.7 | **2.12** (2.11–2.13) |  |
| centroid | points | int | 1,000,000 | 33.4 | 70.2 | **2.11** (2.09–2.11) |  |
| centroid | points | wkb | 1,000,000 | 41.1 | 49.5 | **1.20** (1.20–1.23) |  |
| centroid | poly10 | sep | 100,000 | 21.0 | 32.9 | **1.57** (1.56–1.58) |  |
| centroid | poly10 | int | 100,000 | 22.1 | 34.5 | **1.56** (1.54–1.58) |  |
| centroid | poly10 | wkb | 100,000 | 15.5 | 16.7 | **1.08** (1.08–1.10) |  |
| centroid | poly100 | sep | 100,000 | 135.2 | 192.2 | **1.42** (1.40–1.42) |  |
| centroid | poly100 | int | 100,000 | 149.5 | 205.3 | **1.37** (1.37–1.38) |  |
| centroid | poly100 | wkb | 100,000 | 78.2 | 79.5 | **1.02** (1.00–1.03) |  |
| centroid | poly1000 | sep | 10,000 | 129.4 | 182.1 | **1.41** (1.36–1.41) |  |
| centroid | poly1000 | int | 10,000 | 142.2 | 195.4 | **1.37** (1.37–1.38) |  |
| centroid | poly1000 | wkb | 10,000 | 69.5 | 69.7 | **1.00** (0.98–1.01) |  |
| simplify | points | sep | 1,000,000 | 0.8 | 93.4 | **110.07** (109.91–113.54) | 110.64 |
| simplify | points | int | 1,000,000 | 0.8 | 94.5 | **123.97** (122.71–126.52) | 124.30 |
| simplify | points | wkb | 1,000,000 | 68.0 | 75.4 | **1.11** (1.10–1.11) | 1.10 |
| simplify | poly10 | sep | 100,000 | 79.8 | 97.6 | **1.22** (1.20–1.23) | 1.21 |
| simplify | poly10 | int | 100,000 | 81.0 | 101.6 | **1.26** (1.25–1.27) | 1.24 |
| simplify | poly10 | wkb | 100,000 | 81.8 | 83.8 | **1.02** (0.99–1.03) | 1.03 |
| simplify | poly100 | sep | 100,000 | 898.5 | 950.5 | **1.06** (1.06–1.06) | 1.07 |
| simplify | poly100 | int | 100,000 | 905.7 | 953.1 | **1.06** (1.04–1.06) | 1.07 |
| simplify | poly100 | wkb | 100,000 | 842.7 | 839.7 | **1.00** (0.99–1.02) | 1.00 |
| simplify | poly1000 | sep | 10,000 | 1072.2 | 1120.6 | **1.05** (1.04–1.05) | 1.05 |
| simplify | poly1000 | int | 10,000 | 1080.8 | 1133.9 | **1.05** (1.05–1.06) | 1.05 |
| simplify | poly1000 | wkb | 10,000 | 1008.4 | 1007.0 | **1.00** (1.00–1.00) | 1.00 |
| intersects (relate) | points | sep | 100,000 | 730.2 | 781.5 | **1.07** (1.06–1.08) |  |
| intersects (relate) | points | int | 100,000 | 738.9 | 801.4 | **1.09** (1.07–1.10) |  |
| intersects (relate) | points | wkb | 100,000 | 672.4 | 671.2 | **1.00** (0.99–1.00) |  |
| intersects (relate) | poly10 | sep | 100,000 | 837.2 | 911.0 | **1.09** (1.09–1.09) |  |
| intersects (relate) | poly10 | int | 100,000 | 854.4 | 920.8 | **1.07** (1.07–1.09) |  |
| intersects (relate) | poly10 | wkb | 100,000 | 789.3 | 777.5 | **0.98** (0.98–0.99) |  |
| intersects (relate) | poly100 | sep | 20,000 | 358.4 | 381.8 | **1.07** (1.05–1.07) |  |
| intersects (relate) | poly100 | int | 20,000 | 358.7 | 396.8 | **1.10** (1.09–1.11) |  |
| intersects (relate) | poly100 | wkb | 20,000 | 333.6 | 333.9 | **1.00** (1.00–1.00) |  |
| intersects (relate) | poly1000 | sep | 2,000 | 453.4 | 466.0 | **1.03** (1.02–1.03) |  |
| intersects (relate) | poly1000 | int | 2,000 | 458.1 | 466.6 | **1.02** (1.02–1.03) |  |
| intersects (relate) | poly1000 | wkb | 2,000 | 439.6 | 440.4 | **1.00** (0.99–1.00) |  |
| intersects (trait) | points | sep | 100,000 | 129.3 | 187.4 | **1.45** (1.41–1.46) |  |
| intersects (trait) | points | int | 100,000 | 142.0 | 202.6 | **1.42** (1.42–1.43) |  |
| intersects (trait) | points | wkb | 100,000 | 72.6 | 73.4 | **1.01** (0.99–1.01) |  |
| intersects (trait) | poly10 | sep | 100,000 | 221.9 | 290.8 | **1.31** (1.30–1.36) |  |
| intersects (trait) | poly10 | int | 100,000 | 237.4 | 306.3 | **1.29** (1.28–1.39) |  |
| intersects (trait) | poly10 | wkb | 100,000 | 158.2 | 161.9 | **1.02** (1.02–1.05) |  |
| intersects (trait) | poly100 | sep | 20,000 | 182.1 | 208.7 | **1.15** (1.14–1.16) |  |
| intersects (trait) | poly100 | int | 20,000 | 187.0 | 216.1 | **1.15** (1.13–1.16) |  |
| intersects (trait) | poly100 | wkb | 20,000 | 158.7 | 159.2 | **1.00** (0.99–1.02) |  |
| intersects (trait) | poly1000 | sep | 2,000 | 153.2 | 165.0 | **1.07** (1.07–1.09) |  |
| intersects (trait) | poly1000 | int | 2,000 | 156.5 | 167.3 | **1.07** (1.06–1.07) |  |
| intersects (trait) | poly1000 | wkb | 2,000 | 140.5 | 140.4 | **1.00** (1.00–1.00) |  |
| intersects_const | points | sep | 1,000,000 | 410.8 | 476.3 | **1.15** (1.15–1.16) |  |
| intersects_const | points | int | 1,000,000 | 413.2 | 477.0 | **1.16** (1.15–1.16) |  |
| intersects_const | points | wkb | 1,000,000 | 436.8 | 454.3 | **1.04** (1.03–1.05) |  |
| intersects_const | poly10 | sep | 100,000 | 147.3 | 161.7 | **1.10** (1.09–1.10) |  |
| intersects_const | poly10 | int | 100,000 | 148.7 | 164.5 | **1.10** (1.09–1.12) |  |
| intersects_const | poly10 | wkb | 100,000 | 145.0 | 148.1 | **1.02** (1.02–1.02) |  |
| intersects_const | poly100 | sep | 20,000 | 215.6 | 223.5 | **1.04** (1.03–1.04) |  |
| intersects_const | poly100 | int | 20,000 | 216.9 | 226.5 | **1.04** (1.04–1.05) |  |
| intersects_const | poly100 | wkb | 20,000 | 201.0 | 199.7 | **0.99** (0.98–1.00) |  |
| intersects_const | poly1000 | sep | 2,000 | 432.5 | 439.5 | **1.02** (1.01–1.02) |  |
| intersects_const | poly1000 | int | 2,000 | 432.3 | 440.9 | **1.02** (1.02–1.02) |  |
| intersects_const | poly1000 | wkb | 2,000 | 417.8 | 416.7 | **1.00** (1.00–1.01) |  |

<!-- checksum groups with differing results: 0 -->
