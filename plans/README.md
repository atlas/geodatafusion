# PostGIS parity plan

geodatafusion aims for parity with PostGIS. The functions are divided into groups that share an
implementation mechanism, so that every function in a group is built the same way. Each group
has a detailed sub-plan. The existing functions in a group are its starting point, and each plan
assesses whether they should be restructured first.

- [inventory.md](inventory.md) lists every PostGIS function, its group, whether it's
  implemented, its doc-test parity and its usage rank.
- [hypotheses.md](hypotheses.md) holds the experiments behind the decisions below, with their
  pre-registered rules and results. The write-ups are in [experiments/](experiments/).
- [STYLE_GUIDE.md](../STYLE_GUIDE.md) applies to all groups.
- Progress is measured by the sqllogictest parity suite (`cargo slt`, see
  [`tests/sqllogictests/README.md`](../rust/geodatafusion/tests/sqllogictests/README.md)).
  Baseline: 55 of 582 doc-test records pass.

## Groups

| Group | Mechanism | Functions | Existing basis | Plan |
|---|---|---|---|---|
| G1 | **Native:** implemented from scratch on `geo-traits` / GeoArrow arrays. Also everything that reads or writes Z/M, and anything where neither `geo` nor GEOS matches PostGIS. | 144 (+2 scalar forms) | `udf/native/{accessors,constructors,bounding_box}` | [g1-native.md](g1-native.md) |
| G2 | **`geo` crate:** wrappers around `geo` algorithms, only where `geo` matches PostGIS | 12 | `udf/geo/*` | [g2-geo.md](g2-geo.md) |
| G3 | **External C libraries:** GEOS and PROJ, feature gated | 56 (+2 scalar forms) | `udf/geos/processing/line_merge.rs` | [g3-external-libraries.md](g3-external-libraries.md) |
| G4 | **Serialization and encodings:** text/binary formats, GeoHash, encoded polylines | 44 | `udf/native/io/*`, `udf/geohash/*` | [g4-serialization.md](g4-serialization.md) |
| G5 | **Aggregate, window and set-returning functions** | 29 (+4 aggregate forms) | `bounding_box/extent.rs`, `accessors/dump.rs` | [g5-aggregate-window-set.md](g5-aggregate-window-set.md) |
| G6 | **Types, operators and shared infrastructure:** SQL types, casts, SRIDs, operators, UDF scaffolding | 39 + scaffolding | `data_types.rs`, `error.rs`, `bounding_box/box.rs` | [g6-types-infrastructure.md](g6-types-infrastructure.md) |

A function belongs to the group of its *implementation mechanism*, not its PostGIS chapter.
Aggregates are in G5 even when their per-row algorithm comes from G1 to G3, because the UDF kind
dictates the code shape. Where PostGIS has a scalar and an aggregate with the same name
(`ST_Collect`, `ST_MakeLine`, `ST_Union`, `ST_Polygonize`), the scalar form is in G1 or G3 and
the aggregate in G5.

## Cross-group decisions

The six plans were written in parallel, before the experiments. Where they disagree with this
section, **this section wins**. Each plan keeps its analysis, but its templates are updated by
these decisions. Decisions backed by an experiment cite it.

### Function assignments

| Functions | Group | Decided because |
|---|---|---|
| ST_IsValid, ST_Length | G2 | `geo` matches PostGIS on 100% of the E2 corpus. |
| ST_Perimeter, ST_HausdorffDistance, ST_FrechetDistance, ST_Azimuth, ST_DistanceSphere, ST_DistanceSpheroid, ST_LengthSpheroid, ST_LineLocatePoint | G2, provisional | Not in E2's corpus. Run E2's agreement test before implementing. A function that fails it moves per the backend policy. |
| ST_ConvexHull, ST_OrientedEnvelope, ST_Centroid, ST_PointOnSurface, all spatial predicates (ST_Contains, ST_ContainsProperly, ST_Covers, ST_CoveredBy, ST_Crosses, ST_Disjoint, ST_Equals, ST_Intersects, ST_Overlaps, ST_Touches, ST_Within), ST_Relate, ST_RelateMatch | G3 | `geo` fails E2's agreement test (51–99.95%), and GEOS 3.14.1 with PostGIS's EMPTY rules reaches 100% for the tested ones. The untested predicates follow their family. |
| ST_Area, ST_Distance, ST_DWithin, ST_Simplify, ST_SimplifyVW | G1 | Neither `geo` nor GEOS matches. The differences are a formula (ST_Area: the JTS ring formula matches 100%), tie-breaking (ST_Simplify) and collapse rules (ST_SimplifyVW) (E2). PostGIS computes distances itself. |
| ST_IsValidReason, ST_IsValidDetail, ST_MakeValid, ST_SimplifyPreserveTopology, ST_ConcaveHull, ST_DFullyWithin, ST_IsSimple, ST_IsRing, ST_WrapX, ST_AsMVTGeom, ST_BdPolyFromText, ST_BdMPolyFromText | G3 | PostGIS computes them with GEOS, and the result (or message text) depends on it. |
| 3D measurement family, linear referencing that interpolates Z/M, ST_ChaikinSmoothing, ST_ClosestPoint/ShortestLine/LongestLine/MaxDistance, ST_Angle, ST_PointInsideCircle, ST_OrderingEquals, ST_LineCrossingDirection, ST_SetEffectiveArea, ST_GeneratePoints, ST_GeometricMedian, ST_MinimumBoundingCircle/Radius, ST_MemSize | G1 | `geo` is 2D or lacks the algorithm. ST_MemSize is not rarely used (E6) and has a natural meaning for WKB values. |
| postgis_srs* | G5, blocked on PROJ (G3) | Table functions. |
| ST_AsMVT, ST_AsGeobuf, ST_AsFlatGeobuf | G5, deferred | Aggregates over whole rows. |
| `geometry_dump` struct layout | G5 | Only the dump functions produce it. |
| ST_SRID, ST_SetSRID, Box2D, Box3D, casts, operators | G6 | Type model. |
| GML/KML input, ST_AsX3D, ST_AsMARC21, curve-function shims (ST_HasArc, ST_CurveToLine, ST_LineToCurve, ST_ForceCurve) | Their groups, late phase | Not rarely used (E6). |
| ST_Letters, ST_GeomFromMARC21, ST_ForceSFS, ST_EstimatedExtent | won't do | Bottom 10% by usage and cost a dependency or shim (E6), or GPL data (ST_Letters); catalog statistics (ST_EstimatedExtent). |

### Backend policy

One implementation per SQL function, in the provider whose result matches PostGIS. "Matches"
means E2's test: ≥ 99.9% agreement over its corpus under the parity rules, with every
disagreement category cheaply and deterministically fixable.

1. `geo` (G2), when it matches.
2. Otherwise GEOS (G3), behind the minimum GEOS version feature, when PostGIS uses GEOS. Before
   calling GEOS, reproduce what PostGIS does around it: its EMPTY rules (some EMPTY inputs
   segfault GEOS) and its own point-in-polygon shortcut in ST_Contains/ST_Within/ST_Intersects.
3. Otherwise native (G1), from the PostGIS documentation and observed behaviour.

There's no switching implementations by feature. Python wheels bundle GEOS (E3), so GEOS
functions are available there too.

**Licensing:** don't port or translate PostGIS/liblwgeom (GPL-2.0) or GEOS (LGPL-2.1) source
into this MIT/Apache-2.0 crate. Algorithms from papers, JTS (EDL, BSD-style) and `geo`
(MIT/Apache) may be used with attribution.

### Shared infrastructure

G6 owns crate-wide scaffolding and builds it first (G6 batch 1). Other groups don't add parallel
versions; a missing helper is added to the shared module.

| Concern | Decision | Supersedes |
|---|---|---|
| Location | Crate-wide helpers in `src/util/` (G6 §4). Helpers for one provider in `udf/<provider>/util/`, for one category in `udf/<provider>/<category>/util/`. A helper lives at the narrowest level that covers all its users. | G5's `src/udf/util/` (→ `src/util/`), G1/G2/G3's `data_types.rs` additions |
| Row access | Typed access, never a conversion to WKB for reading (E1: converting native arrays to WKB costs 1.4–460×). The first geometry argument goes through a `GeometryKernel` and the `map_geometry` drivers in `util::kernel` (G1 R1), which dispatch with `downcast_geoarrow_array!` and handle NULL. Further geometry arguments are columns indexed by row, materialised once by typed iteration: `udf::geo::util::GeoColumn` (`GeoValue`), `udf::geos::util::GeosColumn` (GEOS geometries) or `util::owned` (`wkt::Wkt` for native). A constant argument is converted (and prepared) once. | G2's `GeometryArg::iter`, the WKB-backed `GeometryColumn`, G4's `TextWriter`/`write_text_array` (becomes a kernel) |
| Geometry outputs | **WKB** (`geoarrow.wkb`, `Binary`) for every function that returns a geometry, with the CRS of its inputs (E1 H2, E4 H2b/H2c). No `coord_type` on UDFs. Built with `util::kernel::map_geometry_to_wkb`. Box outputs (`box2d`/`box3d`) stay `geoarrow.box`. | Native outputs, `coord_type` fields, G1's `same_type_output` and builder enum |
| Signatures | `single_geometry()` for one geometry argument; otherwise `Signature::user_defined` + `coerce_types` → `coerce_args` with `util::signature::Arg` (G6 §4.1). | G1's `geometry_and`, G2's `ArgKind`, G3's `ArgType` |
| Non-geometry arguments | Read per row with `util::args` readers (broadcast to `args.number_rows`); the kernel reads them by row index. PostGIS `integer` coerces to `Int32`. Only arguments that set the output type or CRS (SRID, `to_proj`) are constant-only, read in `return_field_from_args` with `scalar_srid`/`scalar_text`. | G2's `float_arg(.., len)`, G3's `float64_arg`, G4's `IntArg`/`TextArg` and its `Int64` rule |
| NULL | STRICT semantics: NULL in any argument gives NULL, unless `pg_proc.proisstrict` is false for the PostGIS function. | |
| Return fields | `util::field::geometry_return_field(name, args, geometry_args)`: WKB, CRS from the geometry arguments (mixed SRIDs are an error), named `self.name()`, nullable. | G2's `point_return_field`/`geometry_return_field`, G3's `geometry_return_field` |
| Z/M | Read with `util::ordinates::{z, m}`, never `CoordTrait::nth(2)`. Owned geometries with Z/M are `wkt::Wkt<f64>`, built with `util::owned` (G1 R3). | |
| Return types | PostGIS `integer` → `Int32`, `smallint` → `Int16`, `bigint` → `Int64`, `float8` → `Float64`, `boolean` → `Boolean`, `text` → `Utf8`, `bytea` → `Binary`. Plain types, no GeoArrow tag on text/binary output (E4 H3b). No unsigned integers, no `Utf8View`. | |
| Errors | DataFusion's error macros, messages prefixed with `self.name()` (G6 R3). | G1/G4's "`ST_Name:` prefix" |
| Documentation | `#[user_doc]` with PostGIS chapter sections, and a test that checks every UDF (G6 R4, E5 H5). Migrated in one PR in G6 batch 1. | `Documentation::builder` + `OnceLock` |
| SRIDs | Column-level CRS. Convert only with `util::srid`. Read every CRS form producers write (PROJJSON, `OGC:CRS84`, authority codes, missing GeoParquet `crs` = OGC:CRS84). In memory, write `EPSG:n`/`ESRI:n` authority codes (from a table generated from PostGIS's `spatial_ref_sys`), which round-trip in Arrow hand-offs; expand to full PROJJSON when writing GeoParquet, the one place E4 saw authority codes dropped. Per-row SRIDs that contradict the column are an execution error. | |
| `geo` kernels | Owned by geodatafusion, `geoarrow-expr-geo` dropped, keeping its shortcuts for point inputs (E1 H6, D6). | |
| Macros | `macro_rules!` only for a family of five or more near-identical UDFs in one file, expanding to the standard anatomy (the predicates). | |
| Scalar/aggregate name clash | Drive the upstream DataFusion fallback from a scalar to a same-named aggregate first: the aggregate forms of ST_Union and ST_Collect are 82–88% of their use (E6 H8). `st_<name>_agg` names only if upstream stalls. | G5's interim `_agg` names as the default |

### Harness and CI

- Done: PostGIS `bytea` values are no longer escaped twice.
- Pin GEOS 3.14.1 for parity and CI: `cargo update -p geos-sys --precise 2.0.9` and
  `-p geos-src --precise 0.2.4`, a static (`geos/static`) build, and a Dependabot ignore. With the
  current lock, `geos/static` silently builds GEOS 3.15.1dev (E3 H7a). Cost: +50–92 s per clean
  CI build (E3 H7b).
- PROJ: a `proj` feature with bundled PROJ (+1–1.5 min in CI), `proj-sys` directly for
  pipelines, and `proj.db` shipped or located with `PROJ_DATA` (E5 H12).
- G6 batch 2: run the geodatafusion engine with the PostgreSQL dialect, for the operators.
- G6 batch 5 (DataFusion 55): remove the `::geometry` shim.

## Bugs found in existing code

Fix these in the owning group's first batch, each with a hand-written `.slt` regression test.

| Group | Bug |
|---|---|
| G3 (was G2) | `ST_Contains`/`ST_Within`/`ST_Covers`/`ST_CoveredBy` with a constant first argument return wrong results (argument order swapped in the prepared fast path, `relate.rs:183-208`). Fix now, before the move to GEOS. |
| G2, G3 | `ST_Simplify(g, NULL)` and a NULL constant in any predicate panic; `POINT EMPTY` is an error in every `geo` function. |
| G2, G3 | `ST_Length` (G2) of a GeometryCollection is 0; `ST_SimplifyPreserveTopology` (G3) runs Visvalingam; degenerate convex hulls (G3) are wrong. |
| G1 | `ST_Z` returns M for XYM points; the bounds code treats M as Z; `ST_EndPoint` underflows on `LINESTRING EMPTY`; `ST_MakePointM` panics with interleaved coordinates; `ST_Point(1, NULL)` returns a point; `ST_Point(1, 2, NULL)` panics. |
| G1 | `GeometryType` adds Z/ZM suffixes; `ST_NumPoints` is wrongly an alias of `ST_NPoints`; `ST_IsClosed` ignores Z; `ST_CoordDim` returns values for NULL rows; box functions return ±∞ for EMPTY. |
| G1, G3 | `ST_Envelope`, `ST_Expand`, `ST_PointN` and `ST_ConcaveHull` are marked implemented in the README but aren't registered. |
| G3 | `ST_LineMerge` drops Z; a NULL `directed` is treated as false. |
| G4 | `ST_AsText`/`ST_AsBinary` output carries a GeoArrow tag, which breaks pandas/pyarrow and leaks through `UNION ALL` and `CAST` into unreadable Parquet (E4). |
| G5 | `ST_Extent` fails with partial aggregation (no `state_fields`) and returns an infinite box for no rows. |
| G6 | The GeoParquet reader turns a missing `crs` into no CRS; GeoParquet defines it as OGC:CRS84 (E4). |
| Upstream | DataFusion 54/55: `COALESCE(<Binary>, NULL)` fails to plan ("Expect to get struct but got Binary"), which hits WKB geometry columns; CASE, COALESCE, make_array and array_agg drop extension metadata. geoarrow-rs: one-member GEOMETRYCOLLECTION collapses (an 8-line fix is in `experiments/e7-union-outputs/`), mixed-dimension and `MULTIPOLYGON(EMPTY, ...)` panics (still in 0.9.0). GEOS 3.14/3.15: `ST_Relate('POLYGON EMPTY', 'GEOMETRYCOLLECTION(LINESTRING EMPTY, POINT(1 1))')` segfaults. |

## Phasing

Within each step, work in usage order ([inventory.md](inventory.md), E6).

1. **Foundations:** G6 batch 1 (`src/util/` including the kernel drivers and WKB output, errors,
   `#[user_doc]`, SRID helpers, ST_SRID, ST_SetSRID). In parallel: the predicate
   argument-order bug and the panics, which give wrong answers or crash today.
   *Status:* done, except ST_Extent's partial aggregation, and GeoParquet's PROJJSON
   expansion and missing-`crs` default. The two remaining panics (ST_EndPoint on
   `LINESTRING EMPTY`, ST_MakePointM with interleaved coordinates) went with phase 3's output
   migration. `util::args` readers beyond `scalar_srid`, and `util::ordinates`/`util::owned`,
   are added with their first user.
2. **The biggest unlock:** G4's ST_GeomFromEWKT/ST_AsEWKT and PostGIS number formatting. EWKT
   is the first failure in about 100 records.
   *Status:* done: PostGIS number formatting and ISO/extended WKT writing, a PostGIS-compatible
   (E)WKT parser behind ST_GeomFromText, ST_GeomFromEWKT and ST_AsEWKT. Parity 116/637 ->
   265/773. Most records that used EWKT now fail on functions from other groups instead. Left
   for later G4 batches: EWKB, the type-checked `*FromText` constructors, and routing plain
   text geometry arguments through the new parser.
3. **Migrations:** each group moves its existing functions to its template and output encoding,
   in one breaking release (E6 H3a: no public user breaks). The functions changing backend move
   (E2).
   *Status:* outputs done: every geometry-returning UDF returns WKB with no `coord_type`,
   ST_AsText/ST_AsBinary return untagged Utf8/Binary, and return types map to the PostGIS SQL
   types (D2, D3). The consumers that only read native points were moved to the kernel drivers
   first. Parity 265/773 -> 361/865. Left: the G1 kernel migrations, ST_Extent's
   `state_fields`, the GeoHash rewrite and move, EWKB, the GEOS bridge and backend moves, and
   removing the implicit `geos` feature.
4. **New functions:** group batches in parallel. Pull forward the most-used cheap ones:
   ST_DWithin, ST_Multi, ST_AsGeoJSON/ST_GeomFromGeoJSON. Then ST_Transform (G3, PROJ),
   ST_Buffer/ST_Union/ST_Intersection (G3), ST_Collect/ST_MakeLine (G1, G5). G6 batches 2–4
   (operators, types and casts, geography) unblock parts of G2, G3 and G5.
5. **Late:** GML/KML input, ST_AsX3D, ST_AsMARC21, curve shims.
6. **DataFusion 55** (geoarrow 0.9.0, released 2026-09-11, is on arrow 59): shim removal, `VALUES` metadata. On 55 every cast drops extension metadata (E7), so check nothing relies on casts keeping it.

## Decisions

| # | Decision | Outcome | Basis |
|---|---|---|---|
| D1 | Row access | Typed kernels for the first geometry argument, row-indexed columns for the rest; no WKB conversion for reading. | E1 H1 refuted (conversion cost). |
| D2 | Output encoding | WKB everywhere. The native union (`geoarrow.geometry`) was tested directly (E7): slower in all 5 pipelines (WKB/union 0.68–1.03), no better in SQL on DataFusion 55, and unable by spec to hold nested collections. Not worth revisiting. | E1 H2, E7 H2d–H2f refuted; E4 H2b, H2c hold. |
| D3 | Breaking changes | One release: WKB outputs, signed and plain return types, untagged text/binary, GeoHash moved, implicit `geos` feature removed. | E6 H3a, E4 H3b. |
| D4 | Builds without GEOS | No `geo` fallbacks; wheels bundle GEOS. | E3 H13. |
| D5 | `#[user_doc]` | Migrate. | E5 H5. |
| D6 | `geoarrow-expr-geo` | Drop it, keeping the point shortcuts. H6 failed as written because it re-measured D1's conversion cost; without that confound the owned typed kernels are within 3% on polygons. The only large gap, point inputs in native encoding (area 10x), is a shortcut the owned kernels keep, and with WKB outputs (D2) most inputs are WKB, where the owned kernels are 0.86–1.03. Owning the kernels also puts PostGIS's EMPTY, NULL and dimension rules in geodatafusion, where they belong whatever the backend, and unblocks `geo` 0.33 (`geoarrow-expr-geo` 0.8 pins `geo ^0.31`). Removed as its last users migrate (G2 R1, the E2 backend moves). | E1 H6. |
| D7 | GEOS pinning | Pin 3.14.1 with a static build in CI; floor stays 3.11. | E3 H7a, H7b. |
| D8 | Scalar/aggregate names | Upstream fallback first, `_agg` names only if it stalls. | E6 H8. |
| D9 | CRS form | Read every form. `EPSG:n` in memory, full PROJJSON when writing GeoParquet (your choice of the three options E4 identified; no vendored PROJJSON or PROJ dependency). | E4 H9. |
| D12 | PROJ | `proj` feature. | E5 H12. |
| D13 | GEOS in wheels | Bundle it. **Needs your call:** static (+0.3–0.8 MB) or auditwheel-vendored shared libraries (+1.8 MB, Shapely's model); LGPL-2.1 §6 treats them differently. | E3 H13. |
| D15 | Out of scope | Only ST_Letters, ST_GeomFromMARC21, ST_ForceSFS (and ST_EstimatedExtent). The rest goes to a late phase. | E6 H15. |
| D10 | Mixed-SRID checks | At planning for geometry-returning functions, at execution for fixed-return ones (PostGIS checks at execution). | Principle. |
| D11 | `sql` feature and API | On by default; users install `GeoTypePlanner` via `SessionStateBuilder`; document the PostgreSQL dialect. | API design; **your call**. |
| D14 | Upstream work | File the issues listed under bugs; don't wait for them. | Process; **your call** on who files. |
| D16 | Accessors on the wrong type | Error, like PostGIS. | Principle 1. |

Smaller per-group questions (G2 Q4/Q5/Q7/Q10, G5 Q3/Q5/Q7, G6 Q2/Q4/Q7/Q10/Q12) stay in the plans
and can be decided when the group gets there.
