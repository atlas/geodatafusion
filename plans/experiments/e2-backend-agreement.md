# E2: backend agreement

Tests H4a and H4b from [hypotheses.md](../hypotheses.md). The decision rules below are copied
unchanged from there. Code: [`experiments/e2-backend-agreement/`](../../experiments/e2-backend-agreement/).

## Hypotheses and rules

**H4a (backend policy, G2's functions).** For each contested function, `geo` matches PostGIS on
at least 99.9% of a corpus, after the cheap normalization the plans allow (ring orientation,
start vertex).

- **Rule:** a function stays on `geo` if agreement is ≥ 99.9% and every disagreement category
  has a cheap, deterministic fix. Otherwise it moves to GEOS (or native).

**H4b (G3's premise).** The `geos` crate, linked against the same GEOS version as PostGIS,
matches PostGIS on ≥ 99.99% of the same corpus.

- **Rule:** if it holds, moving functions to GEOS buys parity. If it doesn't, find out what
  PostGIS does around GEOS before any function moves.

## Summary

| Function | geo normalized | GEOS + PostGIS rules | H4a verdict | Recommended backend |
|---|---|---|---|---|
| ST_IsValid | 100.00% | 100.00% | holds | `geo` + zero-area-ring rule (or GEOS if the validity family stays together, see below) |
| ST_PointOnSurface | 51.15% | 100.00% | fails | GEOS |
| ST_ConvexHull | 99.95% | 100.00% | fails (robustness category has no cheap fix) | GEOS |
| ST_OrientedEnvelope | 76.58% | 100.00% | fails | GEOS |
| ST_Simplify | 95.83% | (67.90%, GEOS isn't what PostGIS uses) | fails | native Douglas-Peucker |
| ST_SimplifyVW | 85.22% | n/a | fails | native Visvalingam-Whyatt |
| ST_Centroid | 99.64% | 100.00% | fails | GEOS |
| ST_Area | 99.70% | 100.00% | fails | native (JTS ring-area formula), or GEOS |
| ST_Length | 100.00% | 100.00% | holds | `geo` + collection recursion |
| ST_Distance | 99.83% | 99.87% | fails | native, see below |
| ST_Contains | 98.36% | 99.96% | fails | GEOS + point-in-polygon shortcut |
| ST_Intersects | 99.84% | 99.87% | fails | GEOS + point-in-polygon shortcut |
| ST_Within | 98.52% | 99.96% | fails | GEOS + point-in-polygon shortcut |
| ST_Touches | 99.79% | 100.00% | fails | GEOS |
| ST_Relate | 96.59% | 100.00%* | fails | GEOS, with an EMPTY guard |

\* Two pairs crash both PostGIS and GEOS; counted as agreement because both sides fail.

- **H4a holds for ST_IsValid and ST_Length only.** ST_ConvexHull clears 99.9% but has five
  robustness disagreements without a cheap fix, so under the rule as written it moves to GEOS.
- **H4b does not hold** as stated: pooled over the ten functions PostGIS computes with GEOS,
  raw GEOS agrees on 186432/186510 (99.958%), and 99.968% with PostGIS's EMPTY rules. Per the
  rule we looked at what PostGIS does around GEOS. Two things account for every remaining
  difference: PostGIS's EMPTY shortcuts, which a wrapper copies cheaply, and PostGIS's own
  point-in-polygon shortcut in ST_Contains/ST_Within/ST_Intersects (and its native
  ST_Distance), which GEOS doesn't have. With the EMPTY rules, six of the ten GEOS-computed
  functions agree 100%, and the three predicates fail only on point-vs-polygon cases.
- **New bug:** `ST_Relate('POLYGON EMPTY', 'GEOMETRYCOLLECTION(LINESTRING EMPTY, POINT(1 1))')`
  segfaults GEOS 3.14.1 and 3.15.0 (`geosop` reproduces it) and takes down the PostgreSQL
  backend. A GEOS-backed ST_Relate needs an EMPTY guard before it calls GEOS.

## Method

1. **Corpus** (`gen_corpus.py`): built in PostGIS, in schema `e2`, then exported as WKB.
   Seeded throughout (`random.Random(20261005)`, `setseed`, explicit `ST_GeneratePoints`
   seeds, ids in insertion order). All 2D, SRID 0.
2. **PostGIS results** (`pg_eval.py`): one query per function over the whole table, falling
   back to bisection down to single rows on an error. A lost connection (backend crash) counts
   as an error for that row. Geometries come back as hex WKB, numbers as `float8` text
   (`extra_float_digits = 1`), booleans as text.
3. **Rust backends** (`src/main.rs`, `src/norm.rs`), per case:
   - **geo raw:** what geodatafusion does today: geoarrow-expr-geo's conversion (an EMPTY point
     anywhere is an error), then the kernel as geoarrow-expr-geo calls it (e.g. `Polygon::simplify`,
     ST_Length on lines only, NULL when `centroid()`/`interior_point()`/`minimum_rotated_rect()` return `None`).
   - **geo normalized:** the G2 plan's R6/§7 rules plus the cheap fixes found here (next
     section), on `geo` 0.31.0.
   - **GEOS raw:** the `geos` crate 11.1.1 / `geos-sys` 2.0.9 / `geos-src` 0.2.4 (`static`
     feature), so GEOS 3.14.1 is compiled in and linked statically. At runtime `geos::version()`
     returns `3.14.1-CAPI-1.20.5`, the same string as the oracle's `postgis_full_version()`,
     and `ldd` shows no libgeos. Functions: `is_valid`, `point_on_surface`, `convex_hull`,
     `minimum_rotated_rectangle`, `simplify` (DP), `get_centroid`, `area`, `length`,
     `distance`, `contains`, `intersects`, `within`, `touches`, `relate`. The `geos` crate has
     no plain Visvalingam-Whyatt, so ST_SimplifyVW is "not available".
   - **GEOS + PostGIS rules:** GEOS raw plus the shortcuts PostGIS applies around GEOS: EMPTY in
     → input unchanged (ST_ConvexHull, ST_Simplify), `POLYGON EMPTY` (ST_OrientedEnvelope),
     `true` (ST_IsValid), NULL (ST_Distance), `false` (predicates). A geometry GEOS can't build
     (ring with fewer than 4 points) is invalid. ST_Length sums only the linear parts.
   - Release build, so `geo`'s `debug_assert!`s are off, as in a shipped geodatafusion. Panics
     are caught with `catch_unwind` and counted as disagreements. GEOS relate on inputs with
     EMPTY parts runs in a child process because of the segfault.
4. **Comparison:** every value is rendered with an unmodified copy of the parity harness's
   `tests/sqllogictests/render.rs`: canonical ISO WKT for geometries, floats rounded to 12
   significant digits (`-0` = `0`), booleans and the ST_Relate matrix verbatim. Two values
   agree if the strings are equal, or if both sides raise an error (messages are not compared).
   A panic never counts as agreement.
5. **Classification:** each disagreement gets a kind computed from the rendered strings:
   backend error/panic, NULL vs value, different geometry type, vertex order (same vertex set
   and count), float precision (same structure, every number within 1e-9 relative), different
   vertices (count or coordinates), different value/boolean/matrix. Each also records whether
   an input is or contains EMPTY (E), is invalid in PostGIS (I), or is valid and non-empty (V),
   and whether GEOS agrees with PostGIS on that case. The categories below come from reading
   the smallest examples of each kind.

### Corpus

9471 geometries (unary functions) and 27831 pairs (binary functions).

| Part | Geometries | Contents |
|---|---|---|
| NYC boroughs (`fixtures/geoparquet/nybb_wkb.parquet`) | 5 + 106 parts | Large multipolygons in feet (EPSG:2263 coordinates), 76063 vertices |
| Natural Earth (GeoJSON from `nvkelso/natural-earth-vector`, fetched 2026-10-05) | 1964 | 110m and 50m countries, 50m states/provinces, 50m lakes, 50m rivers, 110m populated places, 110m coastline |
| Synthetic, Python | 5442 | Points; multipoints (random, duplicate, collinear, identical, corners); random-walk lines; special lines (zero length, collinear unordered, repeated points, closed, self-crossing, 2-point); multilines; star polygons with random orientation and start vertex; polygons with holes; rectangles; random vertex order (mostly invalid); hand-shaped invalid polygons (bowtie, zero-area ring, spike, hole outside, hole crossing, self-touching ring, repeated vertices, hole touching shell); disjoint, overlapping and touching multipolygons; collections (mixed, overlapping polygons, nested, EMPTY members); 14 EMPTY forms × 3 |
| Synthetic, derived in SQL | 1954 | `ST_MakeValid` of the invalid ones, `ST_Buffer` (varying `quad_segs`), `ST_GeneratePoints`, `ST_SnapToGrid` (often invalid), rotated rectangles, `ST_Segmentize` (collinear vertices), `ST_Difference` |

Coordinate frames are mixed: a 0–10 integer grid (exact touches and collinearity), ±100
floats, offsets around 1e6/5e6 (large coordinates), a 1e-6 scale, and degrees. Of the
9471 geometries, 1539 are invalid in PostGIS and 136 are or contain EMPTY.

Pairs: 3000 random pairs from the corpus; 1683 synthetic neighbours translated onto each
other; neighbouring Natural Earth countries (890) and states (253) by bbox; populated places
vs countries (793); rivers vs countries (601); borough parts (726); each sampled geometry
against itself, its reverse, its boundary, a vertex, a point interpolated along its boundary,
its centroid, its envelope and a translated copy (13863); 2500 integer-grid pairs; 1522 pairs
with an EMPTY side.

Simplify tolerances are a per-row fraction of the bbox size, chosen by id from {0, 1e-4,
1e-3, 1e-2, 0.05, 0.2, 1}. ST_SimplifyVW uses half the square of such a length, as an area.

### Environment

| | Version |
|---|---|
| PostGIS | 3.6.4, PostgreSQL 18, GEOS 3.14.1-CAPI-1.20.5 at runtime (compiled against 3.13.1), image `docker.io/postgis/postgis:18-3.6` (`fcc669d63392`) |
| `geo` | 0.31.0 (geo-types 0.7.19, geo-traits 0.3.0, wkb 0.9.1) |
| `geos` | 11.1.1, geos-sys 2.0.9, geos-src 0.2.4 → GEOS 3.14.1 static |
| Rust | 1.97.1 |

The same image runs in a private container (port 54330), because ST_Relate crashed the shared
oracle once during this experiment (it recovered on its own). The shared server has no `e2`
schema left.

## Normalizations applied to `geo` ("geo normalized")

From the G2 plan (R6, §7), all cheap and deterministic:

- EMPTY: parts that are EMPTY are dropped on conversion (so `POINT EMPTY` is no longer an
  error); a fully EMPTY input gives PostGIS's result: ST_IsValid `true`, ST_PointOnSurface and
  ST_Centroid `POINT EMPTY`, ST_ConvexHull and ST_Simplify(VW) the input, ST_OrientedEnvelope
  `POLYGON EMPTY`, ST_Area/ST_Length `0`, ST_Distance NULL, predicates `false`, ST_Relate as an
  empty GEOMETRYCOLLECTION.
- ST_IsValid: a ring with zero area is invalid (G2 bug 11).
- ST_Length: sum the linear parts of collections recursively.
- ST_ConvexHull: one distinct input point → POINT; two → LINESTRING **in input order** (new);
  collinear → LINESTRING from the lowest-Y (then X) point to the highest; otherwise a clockwise
  ring starting at the lowest-Y, lowest-X vertex, **after removing exactly collinear vertices**
  (new: `geo`'s quickhull keeps some and sometimes retraces an edge).
- ST_OrientedEnvelope: degenerate hulls → POINT, or a LINESTRING from the lowest-X (then Y)
  point to the highest (new, differs from the hull rule); otherwise `geo`'s rectangle made
  clockwise and started like JTS's `MinimumAreaRectangle`: at the corner where the side
  through the first hull edge (in JTS hull order) begins (new).
- ST_Simplify: DP per line/ring; drop rings with fewer than 4 points, polygons whose shell
  collapsed, and NULL when nothing is left. New: tolerance 0 still removes collinear points
  (`geo` returns the input for epsilon ≤ 0, so use the smallest positive epsilon); a line that
  collapses to two identical points is dropped; if no point was removed, return the input
  unchanged (PostGIS then keeps EMPTY members).
- ST_SimplifyVW: same structure; new: PostGIS removes points with area `< tolerance`, `geo`
  with area `<= epsilon`, so use `tolerance.next_down()`; collapsed lines are kept.

## Results

### Agreement

| Function | n | geo raw | geo normalized | GEOS raw | GEOS + PostGIS rules | geo normalized, valid non-empty inputs |
|---|---|---|---|---|---|---|
| ST_IsValid | 9471 | 9433 (99.60%) | 9471 (100.00%) | 9471 (100.00%) | 9471 (100.00%) | 7796/7796 (100.00%) |
| ST_PointOnSurface | 9471 | 4788 (50.55%) | 4844 (51.15%) | 9471 (100.00%) | 9471 (100.00%) | 4003/7796 (51.35%) |
| ST_ConvexHull | 9471 | 3 (0.03%) | 9466 (99.95%) | 9452 (99.80%) | 9471 (100.00%) | 7791/7796 (99.94%) |
| ST_OrientedEnvelope | 9471 | 0 (0.00%) | 7253 (76.58%) | 9471 (100.00%) | 9471 (100.00%) | 6097/7796 (78.21%) |
| ST_Simplify | 9471 | 7398 (78.11%) | 9076 (95.83%) | 6431 (67.90%) | 6431 (67.90%) | 7589/7796 (97.34%) |
| ST_SimplifyVW | 9471 | 7770 (82.04%) | 8071 (85.22%) | n/a | n/a | 6741/7796 (86.47%) |
| ST_Centroid | 9471 | 9372 (98.95%) | 9437 (99.64%) | 9471 (100.00%) | 9471 (100.00%) | 7794/7796 (99.97%) |
| ST_Area | 9471 | 9405 (99.30%) | 9443 (99.70%) | 9471 (100.00%) | 9471 (100.00%) | 7790/7796 (99.92%) |
| ST_Length | 9471 | 9147 (96.58%) | 9471 (100.00%) | 3850 (40.65%) | 9471 (100.00%) | 7796/7796 (100.00%) |
| ST_Distance | 27831 | 26039 (93.56%) | 27784 (99.83%) | 26596 (95.56%) | 27795 (99.87%) | 21507/21552 (99.79%) |
| ST_Contains | 27831 | 27000 (97.01%) | 27375 (98.36%) | 27819 (99.96%) | 27819 (99.96%) | 21542/21552 (99.95%) |
| ST_Intersects | 27831 | 27410 (98.49%) | 27787 (99.84%) | 27795 (99.87%) | 27795 (99.87%) | 21536/21552 (99.93%) |
| ST_Within | 27831 | 27044 (97.17%) | 27419 (98.52%) | 27820 (99.96%) | 27820 (99.96%) | 21541/21552 (99.95%) |
| ST_Touches | 27831 | 27396 (98.44%) | 27773 (99.79%) | 27831 (100.00%) | 27831 (100.00%) | 21552/21552 (100.00%) |
| ST_Relate | 27831 | 26517 (95.28%) | 26882 (96.59%) | 27831 (100.00%)* | 27831 (100.00%)* | 21487/21552 (99.70%) |

\* Includes 2 pairs where PostGIS and GEOS both crash.

ST_Area, ST_Length, ST_Distance, ST_Simplify and ST_SimplifyVW are computed by liblwgeom, not
GEOS, in PostGIS. Their GEOS columns show whether GEOS would happen to match, not H4b's premise.

### Disagreements, geo normalized

Counts by kind, split into E (an input is or contains EMPTY) / I (an input is invalid) / V
(valid, non-empty).

| Function | Kind | E | I | V |
|---|---|---|---|---|
| ST_PointOnSurface | different point | 47 | 769 | 3781 |
| ST_PointOnSurface | different type/count (EMPTY members) | 18 | 0 | 0 |
| ST_PointOnSurface | float precision | 0 | 0 | 12 |
| ST_ConvexHull | different vertices | 0 | 0 | 5 |
| ST_OrientedEnvelope | float precision (corners) | 18 | 263 | 1254 |
| ST_OrientedEnvelope | vertex order | 3 | 99 | 265 |
| ST_OrientedEnvelope | different rectangle | 2 | 131 | 183 |
| ST_Simplify | different vertex count | 8 | 151 | 178 |
| ST_Simplify | different vertices | 0 | 25 | 20 |
| ST_Simplify | vertex order | 0 | 4 | 9 |
| ST_SimplifyVW | NULL vs value (rings collapsed) | 0 | 210 | 569 |
| ST_SimplifyVW | different vertex count | 8 | 70 | 365 |
| ST_SimplifyVW | different vertices | 1 | 19 | 121 |
| ST_SimplifyVW | EMPTY members dropped | 37 | 0 | 0 |
| ST_Centroid | different point | 1 | 31 | 0 |
| ST_Centroid | float precision | 0 | 0 | 2 |
| ST_Area | different value (zero-area rings) | 0 | 22 | 0 |
| ST_Area | float precision | 0 | 0 | 6 |
| ST_Distance | different value | 0 | 2 | 44 |
| ST_Distance | float precision | 0 | 0 | 1 |
| ST_Contains | different boolean | 6 | 440 | 10 |
| ST_Intersects | different boolean | 0 | 28 | 16 |
| ST_Within | different boolean | 4 | 397 | 11 |
| ST_Touches | different boolean | 0 | 58 | 0 |
| ST_Relate | different matrix | 63 | 816 | 68 |
| ST_Relate | PostGIS crashed | 2 | 0 | 0 |

### Disagreement categories

| Category | Functions (geo normalized unless noted) | Cases | Cheap deterministic fix? |
|---|---|---|---|
| A. EMPTY handling | all `geo` raw (POINT EMPTY is an error; NULL instead of POINT EMPTY; distance 0 instead of NULL) | 38–1516 per function, raw | **Yes**, the rules above remove all of them. Residue: ST_SimplifyVW drops EMPTY members when it removes points (37 E), ST_PointOnSurface on collections with EMPTY members (18 E). Both fixable by rule. |
| B. Vertex order and start vertex | ST_ConvexHull, ST_OrientedEnvelope | hull 8300+ raw → 0; envelope 6500+ raw → 367 | **Yes** for the hull (clockwise, lowest-Y/X start, 2-point input order). Partly for the envelope: the JTS start-corner rule fixes most, the rest are flat or tied rectangles. |
| C. Degenerate output type | ST_ConvexHull, ST_OrientedEnvelope (raw always returns a polygon) | 1015 raw | **Yes** (POINT/LINESTRING mapping). |
| D. Different algorithm | ST_PointOnSurface (geo's interior point ≠ JTS `InteriorPointArea`), ST_SimplifyVW (PostGIS keeps 4 points per ring, so polygons never collapse; `geo` collapses them), ST_OrientedEnvelope ties (equal-area rectangles) | 4628, ~1400, ~316 | **No** normalization. Needs a different implementation (GEOS, or native code). |
| E. Douglas-Peucker details | ST_Simplify | 395 | **No** for `geo`: PostGIS takes the *first* farthest point on ties (`geo` takes the last, `>=`), and treats duplicate and backtracking points differently. A native DP (~40 lines, textbook algorithm) with first-max ties would fix the probed cases. |
| F. Float precision of constructed coordinates | ST_OrientedEnvelope (geo rotates by an angle; GEOS intersects lines in the original coordinates, so corners differ in the 9th–11th digit at 1e6 magnitude), ST_Area (summation order, 6 V), ST_Centroid (2 V) | 1535, 6, 2 | **No.** The harness's 12 significant digits don't absorb it. For ST_Area, the JTS ring formula (shift by `x0`, EDL-licensed), which GEOS uses and which matches PostGIS 100%, is a cheap native fix. |
| G. Near-zero results | ST_Distance (a point interpolated onto a line: PostGIS `3.04e-15`, `geo` exactly `0`), ST_Area of zero-area rings (PostGIS `4.26e-14`, `geo` `0`) | 44 V, 22 I | **No** for `geo`. A relative 12-digit rule can't absorb an absolute `1e-15`. GEOS distance matches PostGIS here. |
| H. Robustness on near-collinear input | ST_ConvexHull (`geo`'s quickhull returns non-convex, retracing rings for nearly collinear points), ST_OrientedEnvelope flat cases, ST_Intersects/ST_Contains with a point a few ulps from the boundary | 5, ~100, 16 | **No.** |
| I. Invalid input | predicates and ST_Relate (`contains(A, A)` is true in GEOS/PostGIS for a bowtie and false in `geo`), ST_Centroid (weights of overlapping parts), ST_Area (zero-area rings), ST_Touches on overlapping multipolygons | 440 contains, 397 within, 816 relate, 58 touches, 31 centroid | **No.** Behaviour on invalid input is algorithm-specific. |
| J. Mod-2 boundary rule and self-overlap | ST_Relate on valid MultiLineStrings whose parts share endpoints (`geo` treats the shared node as boundary, GEOS doesn't), and on GeometryCollections with overlapping polygons or self-overlapping lines (G2 bug 10, which also hits ST_Contains/ST_Within) | 65 V relate, 6 V contains/within | **No.** Unioning collections first would cost a boolean op per row. |
| K. PostGIS point-in-polygon shortcut (GEOS) | ST_Contains, ST_Within, ST_Intersects (and native ST_Distance) with a point or multipoint against a polygon | GEOS: 12, 11, 36, 36 | **Possible but not cheap**: PostGIS doesn't call GEOS for these; its own point-in-ring test differs from GEOS within ~1e-12 of the boundary and for self-intersecting rings. Same-session evidence: `ST_Intersects` = true while `ST_Relate` = `FF2FF10F2` (disjoint) for the same pair. |
| L. GEOS API semantics | GEOS raw: ST_Length (`GEOSLength` returns the perimeter of polygons), ST_Distance (EMPTY → 0, PostGIS NULL), ST_ConvexHull (EMPTY → `GEOMETRYCOLLECTION EMPTY`), ST_Simplify (GEOS's DP repairs polygons and returns `POLYGON EMPTY`/`POINT` where PostGIS returns NULL/`MULTIPOINT`) | 5621, 1199, 19, 3040 | **Yes**, except ST_Simplify: the PostGIS rules remove the first three completely. GEOS DP is a different algorithm from liblwgeom's. |
| M. Crash | ST_Relate with `POLYGON EMPTY` / `MULTIPOLYGON EMPTY` against a collection holding `LINESTRING EMPTY` and a point | 2 | **Yes** for geodatafusion: handle EMPTY before calling GEOS (PostGIS doesn't, and segfaults). Report upstream to GEOS. |

### Examples (WKT as rendered by the harness)

- A. `ST_IsValid('POINT EMPTY')`: PostGIS `true`; geo raw `ERROR: geo crate does not support empty points.`
- B. `ST_ConvexHull('MULTIPOINT((2 7),(4 2))')`: PostGIS `LINESTRING(2 7,4 2)`; geo normalized before the input-order rule `LINESTRING(4 2,2 7)`.
- B. `ST_OrientedEnvelope('LINESTRING(3 7,10 20,5 10)')`: PostGIS
  `POLYGON((3 7,10 20,10.2981651376 19.8394495413,3.29816513761 6.83944954128,3 7))`;
  geo, clockwise from lowest-Y: `POLYGON((3.29816513761 6.83944954128,3 7,10 20,10.2981651376 19.8394495413,3.29816513761 6.83944954128))`.
  The JTS start-corner rule fixes this one.
- C. `ST_ConvexHull('POINT(1 1)')`: PostGIS `POINT(1 1)`; geo raw `POLYGON((1 1,1 1))` (G2 bug 5).
- D. `ST_PointOnSurface('POLYGON((1 7,7 2,2 6,1 7))')`: PostGIS `POINT(4.55 4)`; geo `POINT(3.9375 4.5)`.
- D. `ST_SimplifyVW('POLYGON((3 2,4 8,5 7,8 5,3 2))', t)`: PostGIS `POLYGON((3 2,4 8,8 5,3 2))`; geo normalized NULL (ring collapsed).
- D. `ST_OrientedEnvelope('LINESTRING(8 3,7 3,8 4)')`: PostGIS `POLYGON((8 4,8 3,7 3,7 4,8 4))`; geo `POLYGON((7.5 2.5,7 3,8 4,8.5 3.5,7.5 2.5))` (same area, different rectangle).
- E. `ST_Simplify('LINESTRING(0 0,1 1,1.5 0.2,3 1,4 0)', 0.7)`: PostGIS `LINESTRING(0 0,1 1,4 0)` (first farthest point); `geo` picks `(3 1)` and would give `LINESTRING(0 0,3 1,4 0)`. From the corpus: `LINESTRING(8 5,9 6,9 7,6 7,7 9,3 10,…,8 5)` → PostGIS `LINESTRING(8 5,9 7,3 10,1 3,3 1,8 5)`, geo `LINESTRING(8 5,7 9,3 10,1 3,3 1,8 5)`.
- E. `ST_Simplify('LINESTRING(0 0,0 0,0 0,1 1)', 0)` = `LINESTRING(0 0,0 0,1 1)` in PostGIS (one duplicate kept); geo normalized `LINESTRING(0 0,1 1)`.
- F. `ST_OrientedEnvelope('POLYGON((1069033.22215 5014793.58315,1068968.56783 5014419.2348,1068563.49078 5015029.91342,…))')`: PostGIS corner `1068780.82997 5015174.07952`; geo `1068780.83019 5015174.0796`.
- G. `ST_Distance('LINESTRING(74.5733637711 49.9632905366,107.969210221 116.754983435)', 'POINT(93.1648659182 87.1462948308)')`: PostGIS `3.04482771746e-15`; geo `0`.
- H. `ST_ConvexHull` of a segmentized, self-retracing line (corpus id 9229): PostGIS `POLYGON((0 0,10 20,10 19,4.4 7.8,1 1,0 0))`; geo normalized keeps `3.8 6.6`, which is nearly but not exactly collinear.
- I. `ST_Contains(A, A)` with `A = 'POLYGON((90.2985623768 29.3513585009,87.8909072789 11.934022435,97.3462743571 -5.30498647933,97.4758167371 -19.5680261164,90.2985623768 29.3513585009))'` (self-intersecting): PostGIS/GEOS `true`; geo `false`.
- I. `ST_Touches('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((1 1,1 0,2 0,2 1,1 1)))', 'POINT(1 0.5)')`: PostGIS `true`; geo `false`.
- J. `ST_Relate('MULTILINESTRING((-40.1762888167 -13.8625505113,-26.7451615535 -0.431423247978),(-26.7451615535 -0.431423247978,-14.7932253866 11.5205129189),(-14.7932253866 11.5205129189,-5.73398904105 20.5797492644))', 'POINT(-40.1762888167 -13.8625505113)')`: PostGIS `FF10FFFF2`; geo `FF10F0FF2`.
- K. `ST_Intersects('POLYGON((6.76336451486 7.06168784154,4.11452173628 8.4701025299,2.23663548514 4.93831215846,4.88547826372 3.5298974701,6.76336451486 7.06168784154))', 'POINT(6.15353547454 5.91476622607)')`: PostGIS `true`, GEOS `false`, while PostGIS's own `ST_Relate` of the pair is `FF2FF10F2`.
- L. `ST_Simplify('POLYGON((1020296.9776 157533.653198,1020050.84619 157498.584595,1020211.94501 158848.030396,1020276.6756 158511.771606,1020296.9776 157533.653198))', t)`: PostGIS NULL; GEOS `POLYGON EMPTY`. `ST_Simplify('MULTIPOINT((4 2))', t)`: PostGIS `MULTIPOINT((4 2))`; GEOS `POINT(4 2)`.
- M. `ST_Relate('POLYGON EMPTY', 'GEOMETRYCOLLECTION(LINESTRING EMPTY,POINT(1 1))')`: segfault in PostGIS 3.6.4/GEOS 3.14.1, in the `geos` crate with bundled 3.14.1, and in `geosop` with GEOS 3.15.0. Without the point it returns `FFFFFFFF2`.

## Verdicts

### H4a, per function

"Agreement" means geo normalized over the whole corpus, as the rule says.

| Function | Agreement | ≥ 99.9% | Every category cheaply fixable | Verdict |
|---|---|---|---|---|
| ST_IsValid | 100.00% | yes | yes (A, zero-area rule) | **stays on `geo`** |
| ST_PointOnSurface | 51.15% | no | no (D) | moves |
| ST_ConvexHull | 99.95% | yes | no (H, 5 cases) | moves (rule as written) |
| ST_OrientedEnvelope | 76.58% | no | no (D, F, H) | moves |
| ST_Simplify | 95.83% | no | no (E) | moves |
| ST_SimplifyVW | 85.22% | no | no (D) | moves |
| ST_Centroid | 99.64% | no | no (F, I) | moves |
| ST_Area | 99.70% | no | no (F, G, I) | moves |
| ST_Length | 100.00% | yes | yes (A, recursion) | **stays on `geo`** |
| ST_Distance | 99.83% | no | no (G) | moves |
| ST_Contains | 98.36% | no | no (I, J, H) | moves |
| ST_Intersects | 99.84% | no | no (H/K, I) | moves |
| ST_Within | 98.52% | no | no (I, J, H) | moves |
| ST_Touches | 99.79% | no | no (I) | moves |
| ST_Relate | 96.59% | no | no (I, J) | moves |

**H4a holds for 2 of 15 functions** (ST_IsValid, ST_Length). It fails for 13.

### H4b

- Pooled over the ten functions PostGIS computes with GEOS (IsValid, PointOnSurface,
  ConvexHull, OrientedEnvelope, Centroid, Contains, Intersects, Within, Touches, Relate):
  GEOS raw 186432/186510 = **99.958%**, GEOS + PostGIS rules 186451/186510 = **99.968%**. Both
  are below 99.99%, so **H4b does not hold** as stated.
- Per function, with PostGIS's EMPTY rules: ST_IsValid, ST_PointOnSurface, ST_ConvexHull,
  ST_OrientedEnvelope, ST_Centroid, ST_Touches and ST_Relate agree 100%. The misses are
  ST_Contains 99.957%, ST_Within 99.960% and ST_Intersects 99.871%, all point/multipoint vs
  polygon (category K), half of them on invalid polygons.
- Over all 14 functions GEOS can compute (excluding SimplifyVW), GEOS raw agrees on 95.89% and
  GEOS + rules on 98.71%. The gap is ST_Simplify: liblwgeom's DP isn't GEOS's.
- What the rule's second branch asked for, "what PostGIS does around GEOS": (1) EMPTY
  shortcuts before GEOS (cheap, copied above); (2) a point-in-polygon short-circuit for
  point/multipoint vs polygon in ST_Contains/ST_Within/ST_Intersects (and ST_Covers/
  ST_CoveredBy, by the G2/G3 plans' description, not measured here), plus its own
  point-in-polygon inside ST_Distance; (3) no guard against the GEOS relate crash.

**Comment on the rules.** H4a's second clause makes a single robustness case decisive: ST_ConvexHull
at 99.95% fails because of 5 synthetic, nearly collinear inputs. H4b's pooled 99.99% mixes
functions PostGIS computes with GEOS and functions it doesn't; the per-function numbers above
are more informative. Counting "both error" as agreement (the harness has no other way to
compare a crash) flatters ST_Relate by 2 cases.

## Recommended backend per function

| Function | Backend | Notes |
|---|---|---|
| ST_IsValid | `geo` (H4a) | Needs the zero-area-ring rule and EMPTY → true. The README's move to G3 rests on the reason text of ST_IsValidReason/ST_IsValidDetail, not on ST_IsValid's result. Keeping ST_IsValid on GEOS with its family is also 100%; the maintainer can choose for consistency. |
| ST_Length | `geo` (H4a) | Collection recursion (G2 bug 6). |
| ST_PointOnSurface | GEOS | 100%. A native JTS `InteriorPointArea` port is the D4 alternative; it would have to reproduce JTS exactly. |
| ST_ConvexHull | GEOS | 100% with the EMPTY rule. `geo` + the normalization above is 99.95% if the maintainer relaxes the rule for builds without GEOS. |
| ST_OrientedEnvelope | GEOS | 100% with the EMPTY rule. |
| ST_Centroid | GEOS | 100% raw. |
| ST_Touches, ST_Relate | GEOS | 100%. Guard EMPTY inputs before `relate` (crash). |
| ST_Contains, ST_Within, ST_Intersects | GEOS | 99.96/99.96/99.87%. The remaining gap is PostGIS's point-in-polygon shortcut; reproducing it means a native point-in-ring test that matches liblwgeom (written from observed behaviour, not ported). Recommend GEOS plus documenting the boundary/invalid-polygon difference, or a later native shortcut if the parity tests need it. |
| ST_Simplify | native | Douglas-Peucker with PostGIS's tie-breaking (first farthest point), tolerance 0, the collapse rules and "unchanged → input". Neither `geo` (95.8%) nor GEOS (67.9%) matches. |
| ST_SimplifyVW | native | VW with a minimum of 4 points per ring and 2 per line, removing points with area `< tolerance`. `geo` has no minimum. |
| ST_Area | native | JTS/GEOS ring formula (shift by the first x), \|shell\| − Σ\|holes\|; GEOS matches 100%. PostGIS's own area isn't GEOS, so policy step 3 (native) applies, but the formula is ten lines. |
| ST_Distance | native, or GEOS + rules as a stopgap | GEOS + EMPTY rule: 99.87%, misses only invalid polygons (category K). `geo`: 99.83%, misses near-zero distances (G). PostGIS's distance is liblwgeom's. |

Changes this implies for the plan: G2 keeps only ST_IsValid and ST_Length of the contested set
(plus functions not tested here). ST_ConvexHull, ST_OrientedEnvelope, ST_Centroid and the
predicates move to G3; ST_Simplify, ST_SimplifyVW, ST_Area and ST_Distance become native (G1).
The G2 plan's assumption that ST_ConvexHull/ST_OrientedEnvelope can stay on `geo` with
normalization holds for the hull (99.95%) but not for the envelope.

## Threats to validity

- **Corpus balance.** About 78% of geometries are synthetic, and many are deliberately
  degenerate (near-collinear, grid-snapped, invalid). On the real-world part alone (2075
  geometries, 3263 pairs from Natural Earth and NYC), geo normalized agrees 100% for
  ST_IsValid, ST_ConvexHull, ST_Centroid, ST_Length, ST_Distance, ST_Contains, ST_Intersects,
  ST_Within and ST_Touches; 99.94% for ST_Relate (2), 99.71% for ST_Area (6, float summation),
  99.13% for ST_Simplify, 93.11% for ST_OrientedEnvelope, 85.35% for ST_SimplifyVW and 32.63%
  for ST_PointOnSurface. So the corpus percentages are a pessimistic mix, and the verdicts for
  ST_Centroid, ST_Distance and the predicates turn on synthetic edge cases.
- **Pair construction.** The derived pairs (self, reverse, boundary, interpolated point)
  target boundary cases on purpose and dominate the predicate disagreements. A random
  real-world workload sees far fewer.
- **Normalizations were tuned on this corpus.** The new rules (2-point hull order, envelope
  start corner, DP tolerance 0, VW strictness) came from reading disagreements and are
  supported by direct psql probes, not by reading PostGIS source (GPL). They may not
  generalize; they'd need hand-written slt tests.
- **The automatic kinds are heuristics** on rendered strings (e.g. "float precision" uses a
  1e-9 relative threshold). The categories in the table come from reading the smallest examples
  of each kind, not every case.
- **Prepared geometries.** PostGIS caches prepared geometries when one argument repeats across
  rows (e.g. consecutive country pairs with the same `a`). The comparison used unprepared GEOS
  calls. No disagreement traced back to it, but it wasn't isolated.
- **2D only.** No Z/M inputs. The G2 plan already records that `geo` drops Z (R8).
- **Error messages aren't compared**, and both-error counts as agreement.
- **Natural Earth** was fetched from the GitHub master branch on 2026-10-05 (sha256 prefixes:
  110m countries `6866c877d39cba9c`, 50m countries `3e458fc036ad0a66`, 50m states
  `69a0e06e640b2d50`, 50m lakes `d350b75978b26fe8`, 50m rivers `f286e0ce978fde99`, 110m places
  `a86028b083182b68`, 110m coastline `851f581ff5ffb844`). A later fetch may differ.

## Reproduction

```sh
experiments/e2-backend-agreement/run.sh
```

`run.sh` fetches Natural Earth, starts a private PostGIS container (`e2-postgis`, port 54330,
same image as the oracle), builds the corpus and PostGIS results, runs the comparison and writes
`target/experiments/e2/out/tables.md` (tables plus the two smallest examples of every
function/backend/kind). Step by step:

```sh
cd experiments/e2-backend-agreement
export E2_DATA=$PWD/../../target/experiments/e2 CARGO_TARGET_DIR=$PWD/../../target/experiments/e2
uv run --with 'psycopg[binary]' --with pyarrow python gen_corpus.py   # corpus → schema e2, unary.tsv, pairs.tsv
uv run --with 'psycopg[binary]' python pg_eval.py                      # pg/<function>.tsv
RUSTUP_TOOLCHAIN=1.97.1 cargo run --release [st_relate,...]            # out/per_function/*.json(l)
python3 summarize.py --examples                                        # tables
podman rm -f e2-postgis                                                # clean up
```

Runtime: corpus about 15 s, PostGIS evaluation about 4 min, comparison about 9 min on 16
threads (ST_Distance on `geo` dominates). The data (~560 MB, mostly `pairs.tsv`) stays in
`target/experiments/e2/`.
