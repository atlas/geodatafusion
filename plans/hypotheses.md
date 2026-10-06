# Hypotheses behind the parity plan's decisions

The [decisions in the parity plan](README.md#decisions-for-the-maintainer) should rest on data
where data can decide them. This file states, for each decision, a testable hypothesis, how it
is tested, and the rule that turns the result into a decision. The rules are written *before*
the experiments run, so the results can't steer them.

The thresholds are judgment calls. Every experiment reports raw numbers, so a threshold can be
changed later without re-running anything. Results go to `plans/experiments/<id>.md`, and
experiment code to `experiments/<id>/`, as standalone crates or scripts outside the workspace.

| Experiment | Decisions | Hypotheses |
|---|---|---|
| [E1 performance](experiments/e1-performance.md) | D1, D2, D6 | H1, H2, H6 |
| [E2 backend agreement](experiments/e2-backend-agreement.md) | D4, backend policy, G2/G3 assignments | H4a, H4b |
| [E3 GEOS versions and distribution](experiments/e3-geos.md) | D7, D13, D4 | H7a, H7b, H13 |
| [E4 type model](experiments/e4-type-model.md) | D2, D3, D9 | H2b, H2c, H3b, H9 |
| [E5 dependency cost](experiments/e5-dependencies.md) | D5, D12 | H5, H12 |
| [E6 usage](experiments/e6-usage.md) | D3, D8, D15, prioritisation | H3a, H8, H15 |
| [E7 native union outputs](experiments/e7-union-outputs.md) | D2 (revisit) | H2d, H2e, H2f |

## E1: performance

**H1 (D1, one loop style).** Reading geometry rows through a WKB-backed `GeometryColumn`
(native-array inputs converted to WKB once per batch) costs little compared with a typed,
`downcast_geoarrow_array!`-dispatched kernel.

- Functions, from cheap to expensive: `ST_X`, `ST_NPoints`, `ST_IsEmpty` (native); `ST_Area`,
  `ST_Centroid` (`geo`); `ST_Intersects` with a constant polygon (`geo`, prepared); `ST_Buffer`
  (GEOS).
- Inputs: 1M points; 100k polygons with 10, 100 and 1000 vertices; each as native separated,
  native interleaved and WKB.
- Measures: end-to-end DataFusion query time (`MemTable` scan, `collect`), kernel time, and
  instruction counts (cachegrind), with the overhead given as a ratio.
- **Rule:** adopt the single loop style if the end-to-end overhead is ≤ 10% for every `geo` and
  GEOS function and ≤ 2× for the cheap native accessors, and WKB inputs are no slower than the
  typed path (≤ 5%). If only the cheap accessors exceed it, adopt the single loop style plus
  typed fast paths for the named functions. Otherwise keep a typed kernel for native functions.

**H2 (D2, output encoding).** Native GeoArrow outputs make multi-step pipelines faster than WKB
outputs.

- Pipelines: `ST_X(ST_Centroid(g))`, `ST_Area(ST_Simplify(g, t))`,
  `ST_AsText(ST_Translate(g, 1, 2))`, `ST_Intersects(ST_Buffer(p, r), q)`, on the H1 inputs.
  Also the in-memory size of the native and WKB outputs.
- **Rule:** keep native outputs if they're ≥ 20% faster end to end on at least two of the four
  pipelines. Otherwise WKB outputs win, because they're simpler (see E4).

**H6 (D6, own kernels).** Kernels owned by geodatafusion (`GeoColumn` → `geo`) are as fast as
`geoarrow-expr-geo`'s kernels.

- Functions: `ST_Area`, `ST_Centroid`, `ST_Intersects`, `ST_Simplify`.
- **Rule:** drop `geoarrow-expr-geo` if the owned kernels are within 10% end to end.

## E2: backend agreement

**H4a (backend policy, G2's functions).** For each contested function, `geo` matches PostGIS on
at least 99.9% of a corpus, after the cheap normalization the plans allow (ring orientation, start
vertex).

- Functions: `ST_IsValid`, `ST_PointOnSurface`, `ST_ConvexHull`, `ST_OrientedEnvelope`,
  `ST_Simplify`, `ST_SimplifyVW`, `ST_Centroid`, `ST_Area`, `ST_Length`, `ST_Distance`,
  `ST_Contains`, `ST_Intersects`, `ST_Within`, `ST_Touches`, `ST_Relate`.
- Corpus: real-world geometries (the `fixtures/` NYC boroughs and a public dataset), random
  valid and invalid polygons and lines, and degenerate and EMPTY cases.
- Comparison: the parity harness's rules (EWKT at 12 significant digits, numbers to 12
  significant digits).
- **Rule:** a function stays on `geo` if agreement is ≥ 99.9% and every disagreement category
  has a cheap, deterministic fix. Otherwise it moves to GEOS (or native).

**H4b (G3's premise).** The `geos` crate, linked against the same GEOS version as PostGIS,
matches PostGIS on ≥ 99.99% of the same corpus.

- **Rule:** if it holds, moving functions to GEOS buys parity. If it doesn't, find out what
  PostGIS does around GEOS before any function moves.

## E3: GEOS versions and distribution

**H7a (D7, pinning).** GEOS 3.12 (Ubuntu 24.04, CI), 3.14 (the PostGIS oracle) and 3.15 (local)
give different results for some GEOS operations at the harness's precision.

- Method: a fixed corpus run through buffer, intersection, union, make-valid, simplify (preserve
  topology), point-on-surface, convex hull, line merge and is-valid-reason under each version.
  Use a podman `ubuntu:24.04` container for 3.12.
- **Rule:** pin GEOS for parity tests if any result differs between versions. Otherwise use the
  system GEOS.

**H7b (D7, cost).** Building the bundled GEOS adds ≤ 3 minutes to a clean CI build.

- **Rule:** if it holds, use the static build in CI. If not, pin through a cached GEOS build or
  the CI image.

**H13 (D13, D4, Python).** A Python wheel can bundle GEOS for ≤ 10 MB extra on Linux, and a
build without GEOS loses functions users have today.

- Method: build a wheel with and without the GEOS feature and compare sizes. List the functions
  the published wheels have today that would disappear if moved to GEOS without bundling.
- **Rule:** bundle GEOS if the wheel stays within the budget. If functions would disappear from
  wheels and bundling isn't possible, they keep a `geo` implementation until it is.

## E4: type model

**H2b (D2).** Native outputs break common SQL that WKB outputs don't: `UNION ALL`, `CASE`,
`COALESCE`, `IN (subquery)`, `array_agg`, joins and comparisons across results of different
geometry functions, and mixed with a WKB column.

- Method: a matrix of constructs × output pairs (Point vs Geometry, native vs WKB), plus the
  number of parity records failing for this reason.
- **Rule:** if more than one construct in the matrix fails with native outputs and works with
  WKB, that counts against native outputs in D2, alongside H2's speed numbers.

**H2c (D2).** The geoarrow-rs `GeometryCollection` collapse and mixed-dimension bugs affect
doc-test records with native `GeometryArray` outputs, and none with WKB.

- Method: count the affected records.
- **Rule:** if they affect any records, either use WKB for those outputs or fix them upstream
  first.

**H3b (D3).** GeoArrow-tagged `Utf8`/`Binary` outputs (today's `ST_AsText`/`ST_AsBinary`) break
DataFusion string and binary functions (`||`, `LIKE`, `length`, `upper`, `md5`, `encode`), or
export to Parquet or Python.

- **Rule:** if anything breaks, plain types are a bug fix, not a matter of taste.

**H9 (D9).** Real GeoParquet and GeoArrow producers mostly write the WGS 84 CRS as PROJJSON or
`OGC:CRS84`, not `EPSG:4326`.

- Method: what GDAL, DuckDB, GeoPandas/pyogrio, geoarrow-rs and geodatafusion's own readers write
  and read. Check whether a column tagged `EPSG:4326` round-trips through GeoPandas and DuckDB.
- **Rule:** `util::srid` must map whatever the producers write to 4326. Write the form that
  round-trips through the most tools.

## E5: dependency cost

**H5 (D5).** `#[user_doc]` adds ≤ 5 crates to the build and ≤ 5% to a clean build. It also
supports PostGIS chapter labels as sections and produces the same `Documentation` as the
builder.

- Method: migrate two UDFs in a scratch copy; compare `cargo tree`, clean build time and the
  generated documentation.
- **Rule:** migrate if all of that holds.

**H12 (D12).** The `proj` crate with bundled PROJ builds on Ubuntu 24.04 within 10 minutes, and
its transforms match PostGIS's to 12 significant digits for every `ST_Transform` doc-test record.

- **Rule:** add a `proj` feature if both hold. If accuracy fails, find out why first (PROJ
  version, grids).

## E6: usage

**H15 (D15, prioritisation).** The functions proposed for skipping (XML input, MARC21, X3D,
`ST_Letters`, `ST_MemSize`, curve-function shims) are each in the bottom 10% of PostGIS functions
by usage.

- Method: occurrence counts per function in public code (GitHub code search) and Q&A
  (GIS Stack Exchange), for every function in the inventory.
- **Rule:** skip a function only if it's in the bottom 10% and costs a new dependency or a
  shim. The usage ranking also orders work within each group's phases.

**H3a (D3).** Few downstream projects depend on what the breaking changes touch: unsigned
integer return types, tagged `ST_AsText`/`ST_AsBinary` output, GeoHash names, the implicit
`geos` feature.

- Method: crates.io reverse dependencies, PyPI downloads, and public code using those APIs.
- **Rule:** batch the breaking changes into one release if fewer than five public projects are
  affected. Otherwise deprecate first.

**H8 (D8).** PostGIS's aggregate forms of `ST_Collect`, `ST_MakeLine` and `ST_Union` are used
more than their scalar forms.

- **Rule:** if so, the interim `_agg` naming hurts most users. Prioritise the upstream DataFusion
  fallback over shipping `_agg` names.

## E7: native union outputs (follow-up to D2)

D2 chose WKB outputs. E1 and E4 covered the native union (`geoarrow.geometry`) only alongside
the per-type native outputs. E7 tests a single union output type (one coordinate layout,
separated) directly against WKB.

**H2d (speed).** Union-only outputs make multi-step pipelines faster than WKB outputs.

- Pipelines: E1's four, plus a longer chain, `ST_X(ST_Centroid(ST_Simplify(ST_Translate(g, 1, 2), t)))`,
  and the same chains over polygon inputs. Consumers read with typed access (the D1 decision).
- **Rule:** union outputs are faster if they're ≥ 20% faster end to end on at least two of the
  five pipelines (H2's rule).

**H2e (SQL, DataFusion 55).** DataFusion 55 keeps extension metadata through `CASE`, `COALESCE`,
`make_array`, `array_agg`, `UNION ALL`, `VALUES` and casts, so a union-typed (or WKB-typed) column
keeps its GeoArrow type and CRS in all of them.

- **Rule:** if every construct keeps the metadata, E4's objection to native outputs is removed
  on DataFusion 55. Any construct that drops it keeps native outputs blocked there.

**H2f (collapse bug).** geoarrow-rs's one-member `GEOMETRYCOLLECTION` collapse can be avoided
without forking geoarrow-rs: by a local builder path or a small upstream fix.

- **Rule:** if a workaround of ≤ 200 lines round-trips all 9 affected doc-test geometries
  exactly, the bug isn't a blocker. Otherwise it is, until geoarrow-rs releases a fix.

**Overall rule for D2:** switch to union outputs only if H2d, H2e and H2f all hold. Otherwise
WKB outputs stay.

## Decisions not tested

| Decision | Why |
|---|---|
| D10 mixed-SRID timing | Follows PostGIS's behaviour (errors at execution) where possible, and planning only where the output CRS needs it. |
| D11 `sql` feature and API | API design; no measurement distinguishes the options. |
| D14 upstream work | Process. |
| D16 accessor errors | PostGIS errors (verified); principle 1 decides. |

## Results

All six experiments ran on 2026-10-05; details, raw data and reproduction commands are in
`experiments/` (results) and `../experiments/` (code). "Verdict" is the outcome under the rule as
written. "Decision" is what the plan adopts. Where they differ, the reason is given.

| Hypothesis | Verdict | Key data | Decision |
|---|---|---|---|
| H1 one loop style | **Refuted** | WKB-backed rows on native inputs: accessors 3.4–460×, ST_Area/ST_Centroid 1.4–2.7×, ST_Intersects/ST_Buffer ≤ 1.09× (wall clock). Identical instructions on WKB inputs. The cost is `to_wkb` (~225 instructions per coordinate), not the loop shape. | Typed access (`downcast_geoarrow_array!` behind a shared kernel driver) for every geometry argument, in every group. No conversion to WKB for reading. |
| H2 native outputs faster | **Refuted** | Native ≥ 20% faster on 1 of 4 pipelines (`ST_X(ST_Centroid(points))`, typed consumers). WKB/native 0.58–1.00 otherwise. Sizes within 1% except points (16 vs 25 B/row). | **WKB outputs** for every geometry-returning function. Supported by H2b and H2c. |
| H6 own kernels as fast | **Refuted as written** | In the `GeoColumn` shape on native inputs, 1.6–2.1× slower: H1's conversion cost again. In a typed loop or on WKB inputs, 0.94–1.03×. `geoarrow-expr-geo` short-circuits points (35–120× faster there). | Drop `geoarrow-expr-geo`, keeping its point shortcuts. The confound is H1's conversion, which the D1 decision removes. **Maintainer to confirm** this reading. |
| H4a `geo` matches PostGIS | **Holds for 2 of 15** | ST_IsValid and ST_Length 100% (with the plan's fixes). ST_ConvexHull 99.95% (fails on 5 robustness cases), predicates 96.6–99.8%, ST_Area 99.70%, ST_Distance 99.83%, ST_Centroid 99.64%, ST_Simplify 95.8%, ST_SimplifyVW 85.2%, ST_OrientedEnvelope 76.6%, ST_PointOnSurface 51.2%. | Backends per function as in the plan's function assignments. |
| H4b GEOS matches PostGIS | **Refuted narrowly** | 99.958% raw, 99.968% with PostGIS's EMPTY rules (bar: 99.99%). 7 of 10 functions 100%. The rest is PostGIS's own point-in-polygon shortcut in ST_Contains/ST_Within/ST_Intersects. GEOS segfaults on some EMPTY inputs to ST_Relate. | The rule says find out what PostGIS does around GEOS: done. G3 reproduces the EMPTY rules and the point-in-polygon shortcut before calling GEOS. |
| H7a GEOS versions differ | **Holds** | 3.12 vs 3.14: 1.47% of 10,895 results differ. 3.14 vs 3.15: 7.79% (60 real changes, the rest ring start/orientation). Raw GEOS 3.14.1 vs PostGIS: 8 differences, all EMPTY handling. | Pin GEOS 3.14.1 for parity: `geos-sys` 2.0.9 and `geos-src` 0.2.4 locked with `--precise`, ignored by Dependabot. `geos/static` with the current lock silently builds 3.15.1dev. |
| H7b static GEOS cheap | **Holds** | +50 to +92 s on a clean CI-like build (4 cores, noisy host). GEOS itself builds in ~5 min but in parallel with Rust; it's the critical path (3–5 min) only in jobs with little Rust to compile. | Static, pinned GEOS in CI. |
| H13 wheels can bundle GEOS | **Holds** | +0.30 to +0.83 MB static, +1.78 MB as auditwheel-vendored shared libraries (Shapely's model). Published 0.3.1 wheels have no GEOS. | Bundle GEOS, so no `geo` fallbacks are needed. Static vs shared linking of LGPL code is a licensing call for the maintainer. |
| H2b native outputs break SQL | **Holds** | 9 of 9 constructs fail across different native types and work with WKB. CASE/COALESCE/make_array/array_agg fail even for one native type (DataFusion 54 drops metadata there). | Counts against native outputs (D2: WKB). |
| H2c geoarrow-rs bugs hit doc tests | **Holds** | 9 records with one-member GEOMETRYCOLLECTIONs collapse with native output, 0 with WKB. Panics reproduce in 0.9.0 and on main. | WKB outputs avoid them; still file upstream. |
| H3b tagged text breaks things | **Holds** | DataFusion functions work, but pyarrow/pandas string functions and GeoPandas fail, and the tag leaks through `UNION ALL` with literals and `CAST`, producing unreadable Parquet. | Plain `Utf8`/`Binary` is a bug fix. |
| H9 producers don't write `EPSG:4326` | **Holds** | GeoPandas, DuckDB and pyogrio write full PROJJSON. GDAL/DuckDB omit `crs` (meaning OGC:CRS84). Only full PROJJSON round-trips through every tool. | Read every form; write full PROJJSON. A missing GeoParquet `crs` means OGC:CRS84 (reader bug). |
| H5 `#[user_doc]` cheap | **Holds** | 0 new crates (already in the graph). Crate rebuild −1.3% (debug), +2.0% (release). Generated documentation equals the builder's. Custom section labels work, without descriptions. | Migrate. |
| H12 PROJ builds and matches | **Holds** | Clean bundled PROJ 9.6.2 build 57–93 s in ubuntu:24.04. All 22 ST_Transform doc-test records bitwise equal to PostGIS (PROJ 9.8.1). +13 MB binary, `proj.db` (9.4 MB) needed at runtime. | `proj` feature. Use `proj-sys` directly for pipelines; plan for shipping `proj.db`. |
| H15 skip candidates rarely used | **Rejected as stated** | 4 of 13 candidates in the bottom 10%. The rule licenses skipping ST_Letters, ST_GeomFromMARC21 and ST_ForceSFS. GML/KML input (11th–24th percentile), ST_AsX3D (59th), ST_MemSize (30th) and curve shims (25th–65th) are not rare. | Skip those three. Others go to a late phase. |
| H3a few affected users | **Holds** | 14 public projects use geodatafusion directly; none break; 4 call ST_AsText/ST_AsBinary, 2 see different output (one already strips the tag). | One release for all breaking changes. |
| H2d union outputs faster | **Refuted** | 0 of 5 pipelines. WKB/union 0.68–1.00 (instructions), 0.69–1.03 (wall clock) over 8 inputs; union pipelines with point outputs take 1.27–1.45× as long. The union reader, builder and 28-child array assembly cost ~600 instructions per row. | WKB outputs stay. |
| H2e DataFusion 55 keeps metadata | **Refuted** | Real geoarrow 0.9.0 arrays on DataFusion 55.1.0 (54 as control): only UNION ALL and VALUES keep the extension; CASE, COALESCE, make_array and array_agg drop it on both, for union and WKB alike. On 55 every cast drops it. `COALESCE(<Binary>, NULL)` fails to plan on both (verified on 54 in the harness). | No change from DataFusion 55 alone. File the DataFusion issues. |
| H2f collapse bug avoidable | **Refuted (6 of 9)** | An 8-line upstream patch or a 149-line local builder fixes the 6 one-member collapses. The other 3 are nested collections, which the GeoArrow spec forbids in `geoarrow.geometry`. | Union outputs can never round-trip nested collections. Offer the patch upstream. |
| H8 aggregates used more | **Holds for 2 of 3** | Aggregate share: ST_Union 88%, ST_Collect 82%, ST_MakeLine 27% (≈1,060 files). | Drive the upstream DataFusion scalar→aggregate fallback before shipping `_agg` names. |

The ranked usage of all 297 functions (`../experiments/e6-usage/usage.csv`) orders the work
within each group's phases. Top 10 unimplemented: ST_SetSRID, ST_DWithin, ST_Transform,
ST_Collect, ST_Multi, ST_Union, ST_Intersection, ST_Buffer, ST_AsGeoJSON, ST_MakeLine.
