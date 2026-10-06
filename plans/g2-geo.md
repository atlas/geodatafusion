# G2: wrappers around the `geo` crate

G2 holds the functions whose per-row algorithm comes from the pure-Rust
[`geo`](https://docs.rs/geo/0.31.0) crate. Today they live in `rust/geodatafusion/src/udf/geo/` and
mostly call [`geoarrow-expr-geo`](https://docs.rs/geoarrow-expr-geo/0.8.0) kernels. This plan
proposes to own those kernels in geodatafusion, behind a few shared helpers, so that every G2
function has the same shape and handles NULL, EMPTY and arguments the PostGIS way.

Evidence comes from the parity suite (`cargo slt`, PostGIS 3.6.4 with GEOS 3.14.1), queries
against the live PostGIS, and a scratch binary that ran the same queries through
`geodatafusion::register` (debug build, all features).

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### Final G2 function list (33)

| Category | Functions |
|---|---|
| measurement | ST_Area, ST_Length (alias ST_Length2D), ST_Perimeter (alias ST_Perimeter2D), ST_Distance, ST_DistanceSphere, ST_DistanceSpheroid, ST_LengthSpheroid, ST_HausdorffDistance, ST_FrechetDistance, ST_Azimuth |
| relationships | ST_Contains, ST_ContainsProperly, ST_CoveredBy, ST_Covers, ST_Crosses, ST_Disjoint, ST_Equals, ST_Intersects, ST_Overlaps, ST_Touches, ST_Within, ST_Relate, ST_RelateMatch, ST_DWithin |
| processing | ST_Centroid, ST_ConvexHull, ST_OrientedEnvelope, ST_Simplify, ST_SimplifyVW |
| validation | ST_IsValid |
| lrs | ST_LineLocatePoint |

Geography overloads of ST_Area, ST_Length, ST_Perimeter, ST_Distance, ST_DWithin and ST_Azimuth
are in scope once G6 defines the geography type (Batch 4).

### Reassigned to G1 (native), 34 functions

`geo` is strictly 2D (`geo_traits` → `geo` conversion drops Z and M) and lacks some algorithms.
Where PostGIS's own (liblwgeom) algorithm is small, a native port matches it exactly.

| Functions | Why not `geo` |
|---|---|
| ST_3DClosestPoint, ST_3DDistance, ST_3DDWithin, ST_3DDFullyWithin, ST_3DIntersects, ST_3DLength, ST_3DLongestLine, ST_3DMaxDistance, ST_3DPerimeter, ST_3DShortestLine | `geo` has no 3D. |
| ST_ClosestPoint, ST_ShortestLine, ST_LongestLine, ST_MaxDistance | `geo::ClosestPoint` only targets a `Point`, and there is no geometry-to-geometry nearest/farthest pair. A port of liblwgeom's `lw_dist2d` also reproduces PostGIS's tie-breaking. Shares code with the 3D family. |
| ST_LineInterpolatePoint, ST_LineInterpolatePoints, ST_3DLineInterpolatePoint, ST_LineSubstring, ST_AddMeasure, ST_InterpolatePoint, ST_LocateAlong, ST_LocateBetween, ST_LocateBetweenElevations | PostGIS interpolates Z and M (`ST_LineInterpolatePoint('LINESTRING(1 2 3, 4 5 6, 6 7 8)', 0.5)` = `POINT(3.5 4.5 5.5)`). `geo`'s `LineInterpolatePoint` is deprecated and 2D. |
| ST_ChaikinSmoothing | `geo` handles endpoints differently: `LINESTRING(0 0,8 8,0 16)` gives `LINESTRING(0 0,2 2,6 6,6 10,2 14,0 16)`, PostGIS gives `LINESTRING(0 0,6 6,6 10,0 16)`. PostGIS also interpolates Z/M. |
| ST_Angle, ST_PointInsideCircle, ST_OrderingEquals | Pure coordinate math, no algorithm to wrap. |
| ST_LineCrossingDirection | No `geo` equivalent. PostGIS uses its own algorithm. |
| ST_FilterByM, ST_SetEffectiveArea | M-based. `geo`'s Visvalingam effective areas aren't exposed. |
| ST_GeneratePoints | Random sampling (volatile), no `geo` API. |
| ST_GeometricMedian | No `geo` API (Weiszfeld with M weights). |
| ST_MinimumBoundingCircle, ST_MinimumBoundingRadius | No `geo` API. PostGIS uses its own implementation. |

### Reassigned to G3 (GEOS), 5 functions

| Function | Why |
|---|---|
| ST_SimplifyPreserveTopology | `geo` has no topology-preserving Douglas-Peucker. The current UDF calls `simplify_vw_preserve` (Visvalingam), so both runnable doc tests fail with very different output. |
| ST_IsValidReason | The text must match GEOS: PostGIS says `Self-intersection[150 150]`, `geo` says `exterior ring has a self-intersection`. |
| ST_PointOnSurface | `geo::InteriorPoint` uses a different algorithm: `POINT(62.91666666666667 105)` instead of PostGIS's `POINT(62.5 110)` for the doc example. Alternative: a native port of JTS `InteriorPointArea` (see open questions). |
| ST_ConcaveHull | PostGIS uses GEOS's ratio-based `ConcaveHull`. `geo`'s concave hull is a different algorithm with a different parameter. The existing `native/processing/concave_hull.rs` is dead code (module commented out in `native/mod.rs:7`), yet `README.md:294` marks it ✅. |
| ST_DFullyWithin | Since PostGIS 3.5 it's directional buffer containment: `ST_DFullyWithin('LINESTRING(0 0,10 0)', 'POINT(5 3)', 4)` is true, but false with the arguments swapped. Needs `ST_Buffer`. |

### Not supported yet

- Geography predicates (`ST_Intersects`, `ST_Covers`, `ST_CoveredBy` on geography) need
  spherical-edge topology, which `geo` lacks. Out of scope for G2.
- Geography `ST_Distance`/`ST_DWithin` between non-point geometries: `geo` has geodesic
  point-to-point distance only. PostGIS returns 111315.28003165 for
  `POINT(0 0.5)` to `LINESTRING(1 0, 1 1)`. Needs a port of liblwgeom's geodetic edge distance (L).
- `ST_Area(geog, false)` (sphere): `geo`'s Chamberlain-Duquette area gives 12391399902.07 where
  PostGIS gives 12281281066.69. Needs a port (M).
- `ST_Relate` boundary node rules 2–4: `geo` implements only the OGC Mod-2 rule (1).
- `ST_IsValid(geom, flags)`: `geo` has no ESRI flag.
- Curves (`CIRCULARSTRING`, ...): GeoArrow can't represent them. Two `st_centroid` doc tests stay
  failing.

## 2. Existing basis

All files are under `rust/geodatafusion/src/udf/geo/`. Each UDF is a hand-written
`ScalarUDFImpl` that decodes its input with `from_arrow_array` and calls one `geoarrow-expr-geo`
kernel. The predicates are generated by a macro.

| File | UDFs | Kernel | Doc tests |
|---|---|---|---|
| `measurement/area.rs` | ST_Area | `geoarrow_expr_geo::unsigned_area` | 0/3 |
| `measurement/length.rs` | ST_Length (+ `st_length2d`) | `euclidean_length` | 0/3 |
| `measurement/distance.rs` | ST_Distance | `euclidean_distance` | 0/6 |
| `processing/centroid.rs` | ST_Centroid | `centroid` | 0/3 |
| `processing/convex_hull.rs` | ST_ConvexHull | `convex_hull` | 0/1 |
| `processing/oriented_envelope.rs` | ST_OrientedEnvelope | `minimum_rotated_rect` | 0/2 |
| `processing/point_on_surface.rs` | ST_PointOnSurface | `interior_point` | 3/5 |
| `processing/simplify.rs` | ST_Simplify, ST_SimplifyVW, ST_SimplifyPreserveTopology | `simplify`, `simplify_vw`, `simplify_vw_preserve` | 0/1, 3/3, 0/3 |
| `relationships/topological/relate.rs` | 10 predicates via `impl_relate_udf!` | `relate_boolean` + own `PreparedGeometry` path | see §7 |
| `validation/is_valid.rs`, `is_valid_reason.rs` | ST_IsValid, ST_IsValidReason | `validation::*` | —, 1/3 |
| `relationships/topological/intersects.rs` | none (dead code, not in `mod.rs`) | `intersects` | |

Most doc-test failures are caused by functions from other groups (§5). The ones caused by G2
itself are `st_simplifypreservetopology` (wrong algorithm), `st_pointonsurface` (different point),
`st_isvalidreason` (message text), `st_orientedenvelope` (vertex order) and `st_centroid` (G4's
`ST_AsText` prints 17 instead of 15 significant digits).

### Bugs found

1. **Wrong results for asymmetric predicates with a constant argument.**
   `relate.rs:183-208` prepares the scalar side and computes `array_geom.relate(prepared)`, but
   applies the callback as if the matrix were `(left, right)`. So
   `ST_Contains(<constant polygon>, <point column>)` returns false where the array/array path
   returns true. Affects Contains, Within, Covers and CoveredBy. Tests only covered the symmetric
   `ST_Intersects`, and literal-only queries are constant-folded into the scalar/scalar path.
2. **Panics on user input.** `ST_Simplify(geom, NULL)` panics at `simplify.rs:227`
   (`expect("Non-null epsilon")`). `ST_Contains(<NULL constant>, col)` panics at `relate.rs:187`.
   Both take the host process down when constant folding runs them on the planning thread.
3. **POINT EMPTY is an error everywhere.** `geoarrow_expr_geo::util::to_geo::geometry_to_geo`
   rejects empty points, so `ST_Area('POINT EMPTY')`, `ST_Distance('POINT EMPTY', ...)` and
   `ST_Intersects('POINT EMPTY', ...)` fail with "geo crate does not support empty points".
   PostGIS returns 0, NULL and false.
4. **Other EMPTY differences.** `ST_Distance('LINESTRING EMPTY', 'POINT(1 1)')` = 0 (PostGIS:
   NULL). `ST_Centroid('POLYGON EMPTY')` = NULL (PostGIS: `POINT EMPTY`).
5. **Degenerate hulls.** `ST_ConvexHull('POINT(1 1)')` = `POLYGON((1 1,1 1))` (PostGIS:
   `POINT(1 1)`). For a collinear line, PostGIS returns `LINESTRING(0 0,2 2)`. Same for
   ST_OrientedEnvelope: `POINT(1 1)` gives `POLYGON((1 1,1 1,1 1,1 1,1 1))`. The return type is
   `PolygonType` (`convex_hull.rs:74`, `oriented_envelope.rs:74`), which can't hold the PostGIS
   result.
6. **ST_Length of collections.** `ST_Length('GEOMETRYCOLLECTION(LINESTRING(0 0,3 4), POLYGON(...))')`
   = 0 (PostGIS: 5), because the kernel only measures `Line`/`LineString`/`MultiLineString`.
7. **ST_SimplifyPreserveTopology uses Visvalingam** (`simplify.rs:178-181`), while its docs
   describe Douglas-Peucker.
8. **ST_Simplify keeps collapsed rings.** `geo::Polygon::simplify` keeps at least 4 points per
   ring. PostGIS drops collapsed rings and returns NULL when everything collapses:
   `ST_Simplify('POLYGON((0 0,10 0,10 10,0 10,0 0))', 20)` is NULL, and a small hole disappears
   at tolerance 2.
9. **Z/M silently dropped.** ST_ConvexHull, ST_Simplify and ST_SimplifyVW preserve Z in PostGIS.
   For a native `LineString Z` input, `simplify.rs:207` declares an XY return field, while the
   kernel builds the output with the input's dimension.
10. **GeometryCollections with overlapping polygons** (valid in PostGIS) hit a `debug_assert!` in
    `geo`'s relate (`edge_end_bundle_star.rs:116`), so debug builds (including `cargo slt`) panic.
    Release builds return an unspecified matrix. GEOS 3.14 (RelateNG) answers correctly
    (`0F2FF1FF2`). Still present in geo 0.33.1.
11. **ST_IsValid of a zero-area ring.** `POLYGON((0 0,1 1,2 2,0 0))` is valid in `geo`, invalid in
    PostGIS. 18 other valid/invalid cases agree.

### Inconsistencies between the existing functions

| Topic | Inconsistency |
|---|---|
| Struct docs | No G2 struct has the required one-line `///` comment (e.g. `area.rs:17`, `centroid.rs:17`, `simplify.rs:19`, macro at `relate.rs:22`). |
| Input decoding | `area.rs:68-69` uses `GeoArrowType::from_arrow_field` + `wrap_array`. The others use `from_arrow_array`. `area.rs:67` and `length.rs:74` `unwrap()` the first array. |
| Signatures | `Signature::any(2, ...)` in `distance.rs:28` (static), `simplify.rs:27,86,145` and `relate.rs:30` (stored in a struct field). Wrong argument types fail at runtime with GeoArrow errors (`ST_Intersects(geom, 1)` → "Data not conforming to GeoArrow specification"). The rest use `any_single_geometry_type_input()`. |
| Impl fn names | `oriented_envelope.rs:78` is `convex_hull_impl` (copy-paste), `point_on_surface.rs:78` is `interior_point_impl`, `is_valid_reason.rs:61` is `is_valid_impl`. |
| Documentation | Syntax and argument names disagree: `"ST_Centroid(geometry)"` with argument `g1` (`centroid.rs:61-63`, also convex hull, oriented envelope, point on surface), `"ST_Simplify(geometry, epsilon)"` with `geom`/`tolerance` (`simplify.rs:68-71`), `"ST_IsValid(geomA)"` with `geom` (`is_valid.rs:49-50`). `distance.rs:49-54` isn't rustfmt-formatted. Only `length.rs:62` has an SQL example. No `with_related_udf`. |
| Return fields | `centroid`/`point_on_surface`/`convex_hull`/`oriented_envelope` repeat the same `return_field_impl`. `simplify.rs:198-216` passes typed inputs through with their own coord type and ignores the UDF's `coord_type` (`TODO` at `simplify.rs:59,118,177`). |
| Parameters | `simplify.rs:224-236` reads a scalar with `expect`/`unreachable!` and rejects arrays. `geos/processing/line_merge.rs:99-112` has the cleaner version of the same thing. |
| Predicates | `relate.rs:161-210` repeats the scalar/array arms, recurses for scalar/scalar and `unwrap()`s (`:59,61,173,174,220`). `intersects.rs` duplicates it, with `as_any` and different conversion calls. Both files carry the same three tests. |
| Registration | `Area.into()` / `IsValid.into()` (struct literal) vs `Centroid::default().into()`. |
| Tests | `area.rs:83` is named `test`. Convex hull, oriented envelope, point on surface and validation have no unit tests. No G2 function has a hand-written `slt/geodatafusion/` file. |
| Layout | Predicates sit in an extra `relationships/topological/` level. Other categories are flat. |

Python bindings exist for all 22 UDFs (`python/src/udf/geo/*.rs`), so none are missing.

## 3. Refactoring assessment

### R1. Own the kernels, with one conversion point (do)

The `geoarrow-expr-geo` kernels target generic GeoArrow users, not PostGIS. They bake in
empty-point errors, NULL for empty centroids, `PolygonType` hulls, dropped dimensions and no
`coord_type`. Patching them upstream would still leave PostGIS-specific rules (EMPTY results,
collapse handling, vertex order) to geodatafusion. Owning them also decouples our `geo` version
from geoarrow-rs (`geoarrow-expr-geo` 0.8 pins `geo ^0.31`).

Before (`area.rs:63-71`):

```rust
let array = ColumnarValue::values_to_arrays(&args.args)?
    .into_iter()
    .next()
    .unwrap();
let geo_type = GeoArrowType::from_arrow_field(&args.arg_fields[0])?;
let result = unsigned_area(&geo_type.wrap_array(&array)?)?;
```

After:

```rust
let len = output_len(&args);
let geom = GeometryArg::try_new(&args, 0)?;
let mut builder = Float64Builder::with_capacity(len);
for value in geom.iter(len) {
    match &*value? {
        // SQL NULL in, SQL NULL out.
        GeoValue::Null => builder.append_null(),
        // PostGIS returns 0 for EMPTY.
        GeoValue::Empty(_) => builder.append_value(0.0),
        GeoValue::Geometry(geom) => builder.append_value(geom.unsigned_area()),
    }
}
Ok(ColumnarValue::Array(Arc::new(builder.finish())))
```

The three-armed match makes every function state its NULL and EMPTY behaviour where a reviewer
sees it. That is where most of the current parity bugs come from. The helpers are in §4.

Effort M (15 UDFs, ~30 lines each, plus helpers). Risk low: parity tests and the new
hand-written slt files cover it. Drops the `geoarrow-expr-geo` dependency.

### R2. Type-checked signatures for multi-argument functions (do)

DataFusion's `TypeSignature::Uniform(2, types)` only generates `(T, T)` combinations
(`datafusion-expr-54.0.0/src/type_coercion/functions.rs:878-888`), so it rejects
`(Point, LineString)`. A `one_of` of `Exact` pairs would need 62 × 62 variants and unreadable
errors. Use `Signature::user_defined` with a shared `coerce_types` helper (§4), which is
DataFusion's own route for custom checks (`datafusion-functions` `coalesce`, `greatest`, ...).

Before (`distance.rs:28`):

```rust
static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| Signature::any(2, Volatility::Immutable));
```

After:

```rust
static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

static ARGUMENTS: &[&[ArgKind]] = &[&[ArgKind::Geometry, ArgKind::Geometry]];

// in impl ScalarUDFImpl, after `signature`:
fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
    coerce_args(self.name(), arg_types, ARGUMENTS)
}
```

`coerce_args` maps a `Null` geometry to `Binary` (an all-NULL WKB array), so `ST_Distance(NULL, g)`
is NULL instead of a runtime error. Effort S. Risk low: calls that used to fail at runtime now
fail at planning, which is intended.

### R3. One predicate implementation, correct prepared fast path (do)

Keep a macro for the 11 predicates. DataFusion does the same for function families
(`make_math_unary_udf!` in `datafusion-functions/src/macros.rs`). But the macro only declares
the struct, docs and trait methods, and delegates to one function with a `Predicate` enum:

```rust
predicate_udf!(
    Contains,
    "st_contains",
    Predicate::Contains,
    "Returns true if no points of B lie in the exterior of A, and A and B have at least one interior point in common."
);

fn predicate_impl(args: ScalarFunctionArgs, predicate: Predicate) -> GeoDataFusionResult<ColumnarValue> {
    let len = output_len(&args);
    let left = GeometryArg::try_new(&args, 0)?;
    let right = GeometryArg::try_new(&args, 1)?;

    // Prepare a constant geometry once. PreparedGeometry caches its geometry graph and R-tree.
    // The matrix is always computed as (left, right), so asymmetric predicates stay correct.
    let relate: Box<dyn Fn(&Geometry, &Geometry) -> IntersectionMatrix> = match (&left, &right) {
        (GeometryArg::Scalar(GeoValue::Geometry(a)), GeometryArg::Array(_)) => {
            let a = PreparedGeometry::from(a.clone());
            Box::new(move |_, b| a.relate(b))
        }
        (GeometryArg::Array(_), GeometryArg::Scalar(GeoValue::Geometry(b))) => {
            let b = PreparedGeometry::from(b.clone());
            Box::new(move |a, _| a.relate(&b))
        }
        _ => Box::new(|a, b| a.relate(b)),
    };

    let mut builder = BooleanBuilder::with_capacity(len);
    for (a, b) in left.iter(len).zip(right.iter(len)) {
        match (&*a?, &*b?) {
            // SQL NULL in, SQL NULL out.
            (GeoValue::Null, _) | (_, GeoValue::Null) => builder.append_null(),
            (GeoValue::Geometry(a), GeoValue::Geometry(b)) => {
                builder.append_value(predicate.matches(&relate(a, b)))
            }
            (a, b) => builder.append_value(predicate.empty_result(a.is_empty(), b.is_empty())),
        }
    }
    Ok(ColumnarValue::Array(Arc::new(builder.finish())))
}
```

`Predicate::empty_result` encodes PostGIS (all verified): ST_Disjoint is true, ST_Equals is true
only if both are EMPTY, everything else is false. `Predicate::matches` calls
`IntersectionMatrix::is_contains()` etc., and `matches("T**FF*FF*")` for ContainsProperly
(`expect` is fine for a constant pattern).

In the same change, flatten `relationships/topological/` into `relationships/predicates.rs`
(the macro family) plus one file per new UDF (`relate.rs`, `relate_match.rs`, `dwithin.rs`), and
delete the dead `intersects.rs`. Effort S–M. Risk low. Fixes bugs 1 and 2.

### R4. Non-geometry arguments as arrays (do)

Replace "scalar only, `NotImplemented` for arrays" with broadcast arrays. A scalar becomes a
`len`-row array (cheap), NULL rows give NULL (PostGIS functions are `STRICT`), and per-row
arguments work. Two PostGIS doc examples pass `generate_series(1, 3)` as a parameter.

```rust
let tolerance = float_arg(&args, 1, len)?;
for (value, tolerance) in geom.iter(len).zip(tolerance.iter()) {
    match (&*value?, tolerance) {
        (GeoValue::Null, _) | (_, None) => builder.push_null(),
        // ...
    }
}
```

Effort S. Risk none. Fixes the `ST_Simplify(geom, NULL)` panic.

### R5. Geometry return types and shared return-field helpers (do)

Four files repeat `return_field_impl`. Add `point_return_field` and `geometry_return_field`
(§4). ST_ConvexHull and ST_OrientedEnvelope return `GeometryType`, because PostGIS returns a
Point or LineString for degenerate input. ST_Simplify/ST_SimplifyVW also return `GeometryType`:
a typed output would need a builder per GeoArrow type, and geoarrow-array's
`GeoArrowArrayBuilder` trait is `pub(crate)` (`geoarrow-array-0.8.0/src/trait_.rs:516`). The SQL
value is the same type either way. Effort S. Risk low.

### R6. PostGIS rules on top of `geo` results (do, per function)

- ST_ConvexHull: map `geo`'s degenerate polygons to Point/LineString, return EMPTY input
  unchanged (PostGIS returns `LINESTRING EMPTY` for `LINESTRING EMPTY`), and normalize the ring
  like JTS: clockwise, starting at the lowest-Y (then lowest-X) vertex. `geo` returns CCW with a
  varying start (`POLYGON((10 0,10 10,0 10,0 0,10 0))` vs PostGIS `POLYGON((0 0,0 10,10 10,10 0,0 0))`).
- ST_OrientedEnvelope: same degenerate mapping. In the doc example `geo`'s ring is the reverse
  of GEOS's. Normalize the orientation, then verify the start vertex on more cases. If it can't
  be made to match, move it to G3.
- ST_Simplify/ST_SimplifyVW: simplify each ring/line as a `geo::LineString` (not
  `Polygon::simplify`, which keeps 4 points), then apply PostGIS's collapse rules: drop lines
  with fewer than 2 points and rings with fewer than 4, drop a polygon whose shell collapsed,
  NULL when nothing is left, and support ST_Simplify's `preserveCollapsed`. `geo`'s
  Douglas-Peucker indices match PostGIS in the non-collapsing cases (checked with
  `simplify_idx`).
- ST_Length/ST_Perimeter: sum linear/polygonal parts of collections recursively.
- ST_IsValid: report zero-area rings as invalid (bug 11) until fixed upstream.

### R7. Move out functions `geo` can't match (do, with G3)

ST_SimplifyPreserveTopology, ST_IsValidReason and ST_PointOnSurface move to G3, ST_ConcaveHull
gets fixed in `README.md`, and the dead `native/processing/` files go (G1/G3 decide whether to
reuse them). Until the G3 versions land, keep the current UDFs registered but documented as
differing ("Unlike PostGIS, uses Visvalingam-Whyatt").

### R8. Keep Z/M for vertex-subset algorithms (later)

For ST_Simplify, ST_SimplifyVW and ST_ConvexHull, the output vertices are a subset of the input
vertices, so Z/M can be preserved. Run `geo::SimplifyIdx`/`SimplifyVwIdx` on the 2D
`LineString`, then build the output from the original `geo_traits` coordinates as
`wkt::types::*` values, which carry Z/M and implement `GeometryTrait`. For the hull, look up
vertices by bit pattern. These use the G1 per-array pattern (`impl_<name>` +
`downcast_geoarrow_array!`) because they need the original coordinates. Effort M. Until then,
document "Unlike PostGIS, the result is always 2D."

### R9. Upgrade `geo` to 0.33 (later, after R1)

0.32 adds `Covers` and `ContainsProperly` traits, `distance_within` and `indexed::PreparedGeometry`
(it deprecates `relate::PreparedGeometry`). 0.33 adds `MakeValid`, Voronoi, DBSCAN and k-means,
which G3 and G5 may want. 0.32 introduced a Euclidean distance bug for separable LineStrings,
fixed in 0.33.0 (georust/geo#1499), so skip 0.32. MSRV 1.88 equals ours. It's blocked only by
`geoarrow-expr-geo` pinning 0.31. Two `geo` versions would interoperate (both use `geo-types`
0.7) but compile twice. Effort S, risk low with the parity suite.

### R10. `Intersects` fast path for ST_Intersects/ST_Disjoint (later, benchmark first)

`geo::Intersects` skips the topology graph. It's faster and doesn't trip bug 10, but it's a
second code path with its own edge cases. Decide with a benchmark (constant polygon vs a column
of points, prepared relate vs `Intersects`).

### R11. Generic trait-based UDFs (don't)

A `trait GeoUnary { fn eval(&Geometry) -> T }` with a blanket `ScalarUDFImpl` would remove ~40
lines per UDF. But it hides the anatomy the style guide standardises, conflicts with concrete
Python wrapper structs and per-UDF docs, and the helpers already remove the duplicated logic.

### R12. `catch_unwind` around `geo` calls (don't)

Bug 10 is a `debug_assert!`, so release builds don't panic. Catching panics in library code
hides bugs and needs `AssertUnwindSafe` everywhere. Track it upstream and see open question 5.

## 4. Canonical templates

### Shared helpers

**`rust/geodatafusion/src/udf/geo/util.rs`** (new, provider-level, `pub(crate)`):

```rust
/// One row of a geometry argument, converted to `geo`.
#[derive(Debug, Clone)]
pub(crate) enum GeoValue {
    Null,
    /// A topologically empty geometry, with its type so it can be returned unchanged.
    Empty(EmptyKind),
    Geometry(geo::Geometry),
}

/// The geometry type of an EMPTY value (`Point`, `LineString`, ..., `GeometryCollection`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum EmptyKind { /* one variant per OGC type */ }

/// An empty `geo` geometry of the given kind, for functions that return EMPTY input unchanged.
pub(crate) fn empty_like(kind: EmptyKind) -> geo::Geometry;

impl GeoValue {
    pub(crate) fn is_empty(&self) -> bool;
}

/// A geometry argument: a constant converted once, or an array converted row by row.
pub(crate) enum GeometryArg {
    Scalar(GeoValue),
    Array(Arc<dyn GeoArrowArray>),
}

impl GeometryArg {
    /// Decodes argument `index` with `from_arrow_array`, accepting every GeoArrow encoding.
    pub(crate) fn try_new(args: &ScalarFunctionArgs, index: usize) -> GeoDataFusionResult<Self>;

    /// Yields `len` rows. A scalar is borrowed on every row instead of being cloned.
    pub(crate) fn iter(&self, len: usize)
        -> Box<dyn Iterator<Item = GeoDataFusionResult<Cow<'_, GeoValue>>> + '_>;
}

/// Converts a geo-traits geometry to a `geo` geometry, keeping only X and Y.
///
/// Returns `GeoValue::Empty` for a topologically empty geometry, never `GeoValue::Null`.
/// Empty points inside multi-points and collections are dropped, as `geo` can't represent
/// them and PostGIS ignores them.
pub(crate) fn geometry_to_geo(geom: &impl GeometryTrait<T = f64>) -> GeoValue;

/// The number of rows to produce: the length of any array argument, or 1 if all are scalars.
pub(crate) fn output_len(args: &ScalarFunctionArgs) -> usize;
```

Notes:

- `GeometryArg::iter` boxes a `downcast_geoarrow_array!`-dispatched iterator, so binary
  functions don't need the 16 × 16 two-argument downcast (`geoarrow-expr-geo`'s
  `downcast_geoarrow_array_two_args!` is `pub(crate)` anyway). The dynamic call per row is small
  next to building a `geo::Geometry`.
- `geometry_to_geo` reuses `is_geometry_topologically_empty` (now `pub(crate)` in
  `native/accessors/is_empty.rs`; G6 may move it to a crate-level module). Keep it a
  function, not a trait: `geoarrow-expr-geo`'s `util/to_geo.rs` documents a rustc regression
  (rust-lang/rust#128887) with the trait-based recursive conversion in release builds. Verify
  with `cargo build --release`.
- DataFusion turns a length-1 result into a scalar when all inputs were scalars
  (`datafusion-physical-expr-54.0.0/src/scalar_function.rs:264-270`), so `output_len` is safe.

**`rust/geodatafusion/src/data_types.rs`** (crate-wide; G6 owns this file, see §5):

```rust
/// The kinds of argument a geodatafusion UDF accepts.
pub(crate) enum ArgKind { Geometry, Float, Integer, Boolean, Text }

/// `coerce_types` for `Signature::user_defined`: returns the coerced types for the first
/// overload in `overloads` that matches `arg_types`, otherwise a plan error naming `name`.
/// Geometry accepts `any_geometry_type()`, with `Null` coerced to `Binary`.
/// Float coerces numeric types and `Null` to `Float64`, and so on.
pub(crate) fn coerce_args(name: &str, arg_types: &[DataType], overloads: &[&[ArgKind]])
    -> Result<Vec<DataType>>;

/// Casts argument `index` to `Float64` and broadcasts a scalar to `len` rows.
pub(crate) fn float_arg(args: &ScalarFunctionArgs, index: usize, len: usize)
    -> GeoDataFusionResult<Float64Array>;
/// Like `float_arg`, but `default` for every row if the argument is absent.
pub(crate) fn optional_float_arg(args: &ScalarFunctionArgs, index: usize, len: usize, default: f64)
    -> GeoDataFusionResult<Float64Array>;
// boolean_arg / optional_boolean_arg, int_arg, string_arg likewise.

/// Return field of a UDF returning points: XY `PointType` with the CRS of argument 0.
pub(crate) fn point_return_field(args: &ReturnFieldArgs, coord_type: CoordType)
    -> GeoDataFusionResult<FieldRef>;
/// Return field of a UDF returning any geometry: `GeometryType` with the CRS of argument 0.
pub(crate) fn geometry_return_field(args: &ReturnFieldArgs, coord_type: CoordType)
    -> GeoDataFusionResult<FieldRef>;
```

The `impl` function builds its output with the type from the same helper (via
`GeoArrowType::try_from(args.return_field.as_ref())`), so the declared field and the array
can't disagree (bug 9).

### Rules

- Single geometry argument: `any_single_geometry_type_input()`. More than one argument:
  `Signature::user_defined` + `coerce_types` delegating to `coerce_args`, with a file-level
  `static ARGUMENTS: &[&[ArgKind]]`.
- Import `geo` algorithm traits anonymously (`use geo::Area as _;`), because several share their
  name with the UDF struct (`Area`, `Centroid`, `ConvexHull`, `Simplify`, `Relate`).
- Every per-row match has explicit `Null`, `Empty` and `Geometry` arms. The `Empty` arm carries
  a comment with the PostGIS result.
- Geography overloads branch once at the top of the `impl` function on
  `is_geography(&args.arg_fields[0])` (G6), then use the `Geodesic`/`Haversine` metric spaces
  instead of `Euclidean`.

### Template: unary, fixed return type

```rust
use std::sync::{Arc, OnceLock};

use arrow_array::builder::Float64Builder;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use geo::Area as _;

use crate::data_types::any_single_geometry_type_input;
use crate::error::GeoDataFusionResult;
use crate::udf::geo::util::{GeoValue, GeometryArg, output_len};

/// Returns the area of a polygonal geometry.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Area;

impl Area {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Area {
    fn default() -> Self {
        Self::new()
    }
}

static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl ScalarUDFImpl for Area {
    fn name(&self) -> &str {
        "st_area"
    }

    fn signature(&self) -> &Signature {
        any_single_geometry_type_input()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(area_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns the area of a polygonal geometry.",
                "ST_Area(geom)",
            )
            .with_argument("geom", "geometry")
            .with_related_udf("st_perimeter")
            .build()
        }))
    }
}

fn area_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let len = output_len(&args);
    let geom = GeometryArg::try_new(&args, 0)?;
    let mut builder = Float64Builder::with_capacity(len);
    for value in geom.iter(len) {
        match &*value? {
            // SQL NULL in, SQL NULL out.
            GeoValue::Null => builder.append_null(),
            // PostGIS returns 0 for EMPTY.
            GeoValue::Empty(_) => builder.append_value(0.0),
            GeoValue::Geometry(geom) => builder.append_value(geom.unsigned_area()),
        }
    }
    Ok(ColumnarValue::Array(Arc::new(builder.finish())))
}
```

### Template: unary, geometry return type

Like the above, plus `coord_type`, as in the style guide's `Centroid`:

```rust
fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
    Ok(point_return_field(&args, self.coord_type)?)
}

fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
    Ok(centroid_impl(args)?)
}

fn centroid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let len = output_len(&args);
    let geom = GeometryArg::try_new(&args, 0)?;
    let mut builder = PointBuilder::with_capacity(point_type(&args.return_field)?, len);
    for value in geom.iter(len) {
        match &*value? {
            // SQL NULL in, SQL NULL out.
            GeoValue::Null => builder.push_null(),
            // PostGIS returns POINT EMPTY for EMPTY.
            GeoValue::Empty(_) => builder.push_empty(),
            GeoValue::Geometry(geom) => match geom.centroid() {
                Some(centroid) => builder.push_point(Some(&centroid)),
                None => builder.push_empty(),
            },
        }
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}
```

`point_type(&Field) -> GeoDataFusionResult<PointType>` and `geometry_type(&Field)` live next to
the return-field helpers and return `Internal` if the field isn't the expected type.
`GeometryBuilder::new(geometry_type(&args.return_field)?)` with `push_geometry(Some(&g))`
replaces `PointBuilder` for other geometry outputs.

### Template: geometry plus parameters

```rust
static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

static ARGUMENTS: &[&[ArgKind]] = &[
    &[ArgKind::Geometry, ArgKind::Float],
    &[ArgKind::Geometry, ArgKind::Float, ArgKind::Boolean],
];

impl ScalarUDFImpl for Simplify {
    fn name(&self) -> &str {
        "st_simplify"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }
    // return_type, return_field_from_args, invoke_with_args, documentation as above
}

fn simplify_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let len = output_len(&args);
    let geom = GeometryArg::try_new(&args, 0)?;
    let tolerance = float_arg(&args, 1, len)?;
    let preserve_collapsed = optional_boolean_arg(&args, 2, len, false)?;
    let mut builder = GeometryBuilder::new(geometry_type(&args.return_field)?);
    for ((value, tolerance), preserve_collapsed) in
        geom.iter(len).zip(tolerance.iter()).zip(preserve_collapsed.iter())
    {
        match (&*value?, tolerance, preserve_collapsed) {
            // SQL NULL in, SQL NULL out. PostGIS functions are STRICT.
            (GeoValue::Null, _, _) | (_, None, _) | (_, _, None) => builder.push_null(),
            // PostGIS returns EMPTY input unchanged (see the note below).
            (GeoValue::Empty(kind), ..) => builder.push_geometry(Some(&empty_like(*kind)))?,
            (GeoValue::Geometry(geom), Some(tolerance), Some(preserve_collapsed)) => {
                match simplify_geometry(geom, tolerance, preserve_collapsed) {
                    Some(simplified) => builder.push_geometry(Some(&simplified))?,
                    // PostGIS returns NULL when every component collapsed.
                    None => builder.push_null(),
                }
            }
        }
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}
```

ST_Simplify, ST_SimplifyVW and ST_ConvexHull return EMPTY input unchanged, which is why
`GeoValue::Empty` carries an `EmptyKind`. Functions that don't care match `GeoValue::Empty(_)`.

### Template: binary

```rust
fn distance_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let len = output_len(&args);
    let left = GeometryArg::try_new(&args, 0)?;
    let right = GeometryArg::try_new(&args, 1)?;
    let mut builder = Float64Builder::with_capacity(len);
    for (a, b) in left.iter(len).zip(right.iter(len)) {
        match (&*a?, &*b?) {
            // SQL NULL in, SQL NULL out.
            (GeoValue::Null, _) | (_, GeoValue::Null) => builder.append_null(),
            // PostGIS returns NULL if either geometry is EMPTY.
            (GeoValue::Empty(_), _) | (_, GeoValue::Empty(_)) => builder.append_null(),
            (GeoValue::Geometry(a), GeoValue::Geometry(b)) => {
                builder.append_value(Euclidean.distance(a, b))
            }
        }
    }
    Ok(ColumnarValue::Array(Arc::new(builder.finish())))
}
```

Predicates use `predicate_udf!` and `predicate_impl` (R3). ST_Relate and ST_DWithin are
hand-written but reuse the same prepared-geometry closure.

## 5. Dependencies

### Crate APIs (verified in `~/.cargo/registry/src`, versions from `Cargo.lock`)

| Crate | API | Used by |
|---|---|---|
| geo 0.31.0 | `Area::unsigned_area` | ST_Area |
| | `Euclidean` (`Distance`, `Length`, `Bearing` in degrees) | ST_Distance, ST_DWithin, ST_Length, ST_Perimeter, ST_Azimuth |
| | `Centroid` (highest dimension of a collection, matches PostGIS) | ST_Centroid |
| | `ConvexHull`, `MinimumRotatedRect` | ST_ConvexHull, ST_OrientedEnvelope |
| | `Simplify`/`SimplifyIdx`, `SimplifyVw`/`SimplifyVwIdx` | ST_Simplify, ST_SimplifyVW |
| | `Validation::is_valid` | ST_IsValid |
| | `Relate`, `relate::IntersectionMatrix` (`is_*`, `matches`, `get`, `FromStr`), `PreparedGeometry` | predicates, ST_Relate, ST_RelateMatch |
| | `line_measures::FrechetDistance` (`LineString` only, discrete; doc example matches 70.71) | ST_FrechetDistance |
| | `LineLocatePoint` for `LineString` | ST_LineLocatePoint |
| | `GeodesicArea::{geodesic_area_unsigned, geodesic_perimeter}`, `Geodesic` (`Distance`, `Length`, `Bearing`) | geography overloads. Matches PostGIS to 12 digits: 12308778361.469452 vs …454, 156899.56829134026 vs …029 |
| | `HaversineMeasure::new(6371008.7714150598)` | sphere distance. PostGIS's radius gives 157249.5977685051, PostGIS 157249.59776851 |
| | `GeodesicMeasure::new(a, f)` | custom spheroid. The second parameter is named `inverse_flattening` but is passed to `geographiclib_rs::Geodesic::new(a, f)` as the flattening |
| geographiclib-rs 0.2.5 (transitive) | `Geodesic::new(a, f)`, `InverseGeodesic` | ST_DistanceSpheroid, ST_LengthSpheroid, if used directly (add to `[workspace.dependencies]`) |
| geo-traits 0.3.0 | `to_geo::{ToGeoPoint::try_to_point, ToGeoLineString, ToGeoPolygon, ...}` | `geometry_to_geo` |
| wkt 0.14.0 | `types::{Coord, LineString::new(coords, dim), ...}` implement `GeometryTrait` with Z/M | R8 |
| geoarrow-array 0.8.0 | `array::from_arrow_array`, `downcast_geoarrow_array!`, `GeoArrowArrayAccessor::iter`, `PointBuilder::{with_capacity, push_point, push_empty, push_null}`, `GeometryBuilder::{new, push_geometry, push_null}` | helpers |
| geoarrow-schema 0.8.0 | `Metadata::edges()` (`Edges::{Spherical, Karney, ...}`), `GeoArrowType: TryFrom<&Field>` | geography detection, output types |
| datafusion 54.0.0 | `Signature::user_defined`, `ScalarUDFImpl::coerce_types`, `ColumnarValue::{cast_to, into_array}`, `ReturnFieldArgs::scalar_arguments` | signatures, parameters |

Not used: `geo::HausdorffDistance`, which is vertex-to-vertex (41.23 for the doc example, where
PostGIS/GEOS give 37.26). ST_HausdorffDistance implements GEOS's discrete Hausdorff with `geo`
primitives: the maximum over each geometry's vertices of `Euclidean.distance(&point, &other)`,
plus GEOS's `densifyFrac` (each segment split into `ceil(1 / frac)` parts). Also not used:
`geo::InteriorPoint`, `geo::ChaikinSmoothing`, `geo::LineInterpolatePoint` and
`geo::ConcaveHull` (§1).

### Upgrades and upstream contributions

- Remove `geoarrow-expr-geo` after R1, then upgrade `geo` to 0.33 (R9).
- geo: relate on GeometryCollections with overlapping polygons (bug 10), zero-area rings in
  `Validation` (bug 11), the `GeodesicMeasure::new` parameter name, and optionally a
  GEOS-compatible Hausdorff mode.
- geoarrow-rs: a public type-generic builder (make `GeoArrowArrayBuilder` public, or add
  `from_type`), so outputs can keep the input's GeoArrow type.

### Other groups

| Group | G2 needs | Unblocks |
|---|---|---|
| G6 | `coerce_args`/`ArgKind`, argument and return-field helpers in `data_types.rs` (G2 adds them in Batch 1 if G6 hasn't). The geography marker (which `Edges` value) and `is_geography(field)`. The SRID model and a mixed-SRID error helper (PostGIS: `ST_Distance: Operation on mixed SRID geometries`). Fixing `FROM generate_series(...) x1` naming columns `x1.value`. | All geography doc tests (st_area, st_distance, st_length, st_perimeter, st_intersects, st_covers), st_isvalidreason/st_linelocatepoint |
| G4 | `ST_GeomFromEWKT`, `ST_AsEWKT`, `ST_GeomFromText(text, srid)`, `ST_GeogFromText`, and `ST_AsText` with PostGIS's 15 significant digits | st_area, st_distance, st_length, st_perimeter, st_distancesphere/spheroid, st_centroid (the only remaining diff), and float noise in hull, oriented envelope and LRS outputs (`3.000000000000001` prints as `3`) |
| G3 | `ST_Buffer` (blocks 10 G2 doc tests), `ST_Transform`, `ST_Union`, `ST_Intersection`. Takes over the 5 functions in §1 | st_contains, st_within, st_covers, st_coveredby, st_containsproperly, st_simplify, st_relate, st_area, st_distance, st_length |
| G5 | `ST_Collect` | st_convexhull, st_orientedenvelope |
| G1 | `ST_Reverse`, `ST_Dimension`, `ST_ExteriorRing`, `ST_Boundary`, `ST_LineInterpolatePoint`, `ST_MakeLine`. Takes over the 34 functions in §1. Shares `is_geometry_topologically_empty` | st_equals, st_overlaps, st_contains, st_linelocatepoint |

## 6. Phasing

Doc-test counts are records that should pass once the batch and its listed blockers land.

**Batch 1: refactor (no new functions).** One PR per step.

1. Helpers (`udf/geo/util.rs`, `data_types.rs` additions) with unit tests.
2. Measurement: ST_Area, ST_Length (collections), ST_Distance (EMPTY → NULL, typed signature).
3. Predicates: `predicate_udf!` + `predicate_impl`, flatten `relationships/`, delete `intersects.rs`.
   Fixes bugs 1–2.
4. Processing: ST_Centroid, ST_ConvexHull and ST_OrientedEnvelope (`GeometryType`, degenerate
   cases, vertex order), ST_Simplify/ST_SimplifyVW (parameters, collapse rules,
   `preserveCollapsed`, `coord_type`).
5. Validation: ST_IsValid (zero-area rings).
6. Coordinate R7 with G3 (SimplifyPreserveTopology, IsValidReason, PointOnSurface). Fix
   `README.md` for ST_ConcaveHull. Remove `geoarrow-expr-geo`.
7. A hand-written `slt/geodatafusion/<function>.slt` for every remaining function, and
   `cargo slt --update-parity`.

No doc-test change expected, except `st_orientedenvelope` +1 after G4's `ST_AsText` fix. Fixes
every bug in §2 except 9 (R8) and 10.

**Batch 2: new geometry functions, no blockers.** ST_Perimeter/ST_Perimeter2D, ST_DWithin,
ST_ContainsProperly, ST_Relate (2- and 3-argument, rule 1 only), ST_RelateMatch,
ST_HausdorffDistance, ST_FrechetDistance, ST_Azimuth, ST_LineLocatePoint. Unlocks 7 doc tests
directly (st_relatematch 2, st_hausdorffdistance 2, st_frechetdistance 1, st_relate 1,
st_azimuth 1), plus st_perimeter 2 after G4.

**Batch 3: dependency upgrade and spheroid functions.** `geo` 0.33 (R9). ST_DistanceSphere,
ST_DistanceSpheroid and ST_LengthSpheroid (points and lines, `SPHEROID["name",a,rf]` parsing).
Z/M preservation (R8). +2 doc tests after G4.

**Batch 4: geography (after G6).** Geography overloads of ST_Area (spheroid), ST_Length,
ST_Perimeter, ST_Distance and ST_DWithin (point-point), and ST_Azimuth, plus mixed-SRID errors.
+4–6 doc tests, depending on G3's `ST_Transform`.

**Batch 5: later (L).** Geodetic distance for non-point geography, sphere area, ST_IsValid
flags, R10.

Of the 59 doc-test records for the final G2 list, 10 pass today. After Batches 1–4 and the
listed blockers, about 45 should pass. The rest need curves, geography predicates or
non-default boundary node rules.

## 7. Per-function notes

S/M/L is the effort. Doc tests are current/total.

| Function | API | PostGIS gotchas | Size | Doc tests |
|---|---|---|---|---|
| ST_Area | `Area::unsigned_area`; geography: `geodesic_area_unsigned` | EMPTY → 0 (POINT EMPTY errors today). Collections sum polygonal parts (matches). Geography `use_spheroid=false` needs a port. | S | 0/3 (G4 EWKT, G3 transform, G6) |
| ST_Length, ST_Length2D | `Euclidean.length` on linear parts; geography: `Geodesic.length` | Polygons → 0, collections sum linear parts recursively (bug 6). 2D even for Z input (PostGIS: 5 for a 3-4-12 line, ST_3DLength 13). | S | 0/3, — (G4 srid, G3, G6) |
| ST_Perimeter, ST_Perimeter2D | `Euclidean.length` of rings; geography: `geodesic_perimeter` | Lines → 0, EMPTY → 0, collections sum polygonal parts (PostGIS: 4). | S | 0/4, — (G4 srid, G6) |
| ST_Distance | `Euclidean.distance(&Geometry, &Geometry)`; geography: `Geodesic.distance` (points) | EMPTY → NULL. Mixed SRID → error (G6). Geography `use_spheroid=false` uses `HaversineMeasure::new(6371008.7714150598)`. Non-point geography is L. | S (+M geog) | 0/6 (G4, G3, G6) |
| ST_DistanceSphere | `HaversineMeasure::new(..)` | Radius from the SRID's spheroid (WGS84 default). Point-point first, others `NotImplemented`. | S | 0/1 (G4 srid, G3) |
| ST_DistanceSpheroid | `geographiclib_rs::Geodesic::new(a, f)` | Parse `SPHEROID["WGS 84",6378137,298.257223563]` (inverse flattening). Point-point first. | M | 0/1 (G4, G3) |
| ST_LengthSpheroid | same | Sums geodesic segment lengths. Polygons give the perimeter. The docs say 2D or 3D, so check Z handling in psql. | M | — |
| ST_HausdorffDistance | `Euclidean.distance(&Point, &Geometry)` over vertices | Not `geo::HausdorffDistance` (vertex-to-vertex). `densifyFrac` must be in (0, 1]. EMPTY → NULL. Doc example 2 (densify 0.5) gives 70, 14.142… without. | M | 0/2 |
| ST_FrechetDistance | `Euclidean.frechet_distance` | Build a `LineString` from all coordinates of any geometry, as GEOS does (polygon vs line = 1.414…). `densifyFrac` default -1 = off. EMPTY → NULL. | M | 0/1 |
| ST_Azimuth | `Euclidean.bearing` → radians; geography: `Geodesic.bearing` | Non-points → error "Argument must be POINT geometries". Coincident points → NULL. Result in [0, 2π). | S | 0/1 |
| 11 predicates | `Relate` + `PreparedGeometry` | EMPTY: Disjoint true, Equals true iff both EMPTY, others false. Bug 1 (argument order). GeometryCollections with overlapping polygons (bug 10). No geography. ContainsProperly via `matches("T**FF*FF*")`. | S (in R3) | Contains 0/2, ContainsProperly 0/2, CoveredBy 0/1, Covers 0/2, Crosses —, Disjoint 2/2, Equals 1/2, Intersects 0/1, Overlaps 2/3, Touches 2/2, Within 0/1 (G3 buffer/union, G1 reverse/dimension/boundary, G6) |
| ST_Relate | `IntersectionMatrix::get` → 9-char string; `matches(pattern)` | Overloads (geom, geom) → text, (…, text) → boolean, (…, integer) → text. Return type depends on the third argument's type. Rules 2–4 `NotImplemented`. Bad pattern → `Execution` error. EMPTY gives a normal matrix (`FFFFFF0F2`): relate an empty `GeometryCollection`. | M | 0/3 (ex. 1 G3 buffer, ex. 3 rule 2) |
| ST_RelateMatch | `IntersectionMatrix::from_str` + `matches` | Text-only. Check that `from_str` accepts every matrix character. Could also be a native 20-liner. | S | 0/2 |
| ST_DWithin | `Euclidean.distance(a, b) <= d` with a bbox prefilter; geography: geodesic (points) | Negative distance → error "Tolerance cannot be less than zero". EMPTY → false. Use `distance_within` after R9. | S | — |
| ST_Centroid | `Centroid` | EMPTY → `POINT EMPTY` (`push_empty`). Result 2D (PostGIS too). Curves unsupported. | S | 0/3 (G4 AsText digits, curves) |
| ST_ConvexHull | `ConvexHull` | Degenerate → Point/LineString. EMPTY → input unchanged. JTS ring order. Z preserved in PostGIS (R8). `GeometryType` output. | M | 0/1 (G5 collect) |
| ST_OrientedEnvelope | `MinimumRotatedRect` | Degenerate → Point/LineString, EMPTY → `POLYGON EMPTY`. Reverse `geo`'s ring and verify the start vertex. Float noise needs G4's 15-digit `ST_AsText`. | M | 0/2 (G4, G5) |
| ST_Simplify | `Simplify` on each `LineString` (later `SimplifyIdx`) | Collapse rules (bug 8), optional `preserveCollapsed`, points unchanged, NULL tolerance → NULL, Z/M kept (R8). | M | 0/1 (G3 buffer) |
| ST_SimplifyVW | `SimplifyVw` per `LineString` | PostGIS keeps 4 points per ring (`POLYGON((0 0,10 10,0 10,0 0))` at tolerance 1000). Check collapse with hand-written tests. Z/M (R8). | M | 3/3 |
| ST_IsValid | `Validation::is_valid` | EMPTY → true. Zero-area ring (bug 11). `flags` → `NotImplemented`. | S | — |
| ST_LineLocatePoint | `LineLocatePoint` | First argument must be a LineString (error "1st arg isn't a line", also for MultiLineString), second a Point. A zero-length line returns 1 in PostGIS. Geography overload later. | S | 0/2 (G1 LineInterpolatePoint, G6 generate_series) |

## 8. Testing

- **Hit every argument shape.** `SELECT f('...'::geometry, '...'::geometry)` is constant-folded,
  so it only exercises the scalar/scalar path. That is how bug 1 went unnoticed. For every
  binary function, add queries over `FROM (VALUES (...), (...)) AS t(a, b)` with a column on
  each side and a constant on the other, in both orders.
- **NULL in every position:** a `NULL` literal (typed `Null`), a NULL row in a column, and a NULL
  parameter (tolerance, pattern, distance). Never a panic.
- **EMPTY of each type**, including `POINT EMPTY` and a collection holding an empty member.
  `MULTIPOINT(EMPTY, 1 1)` doesn't parse yet (G4), so use `GEOMETRYCOLLECTION(POINT EMPTY, POINT(1 1))`.
- **Z/M input:** assert the 2D result, which is what PostGIS records for the measures and
  ST_Centroid. For hull and simplify, keep the queries even though they fail until R8, so the
  gap shows in `parity.txt`.
- **Degenerate input:** single point, collinear points, zero-length line, collapsed rings,
  invalid polygons (bowtie). For predicates, add GeometryCollections with overlapping polygons
  only once bug 10 is handled, because debug builds panic.
- **Compare geometries, not text.** Geometry columns render as canonical EWKT with 12
  significant digits, so float noise from `geo` doesn't matter. `ST_AsText` is compared
  verbatim and belongs to G4's tests. Where only topology matters and vertex order is
  implementation-defined, test with `ST_Equals(f(g), '...'::geometry)` or `ST_NPoints`. The
  expected value is still recorded from PostGIS.
- **Floats:** R columns are rounded to 12 significant digits. Don't round in the implementation
  to make a test pass. In unit tests use `approx::assert_relative_eq!`. Use `assert_eq!` only
  for exactly representable values.
- **Unit tests** (one per UDF, `#[tokio::test] async fn test_<behaviour>()`): return field type
  and CRS propagation, `coord_type` honoured, and each GeoArrow encoding as input (WKB, WKT,
  native, `GeometryType`) via `RecordBatch`es. For predicates, also the prepared path with a
  constant on either side for an asymmetric predicate.
- **Helpers** get their own unit tests in `udf/geo/util.rs`: `geometry_to_geo` on every
  geometry type and empty variant, and `GeometryArg::iter` on scalar, array and NULL scalar.

## 9. Style guide amendments

1. **Layout:** allow a provider-level `util` module (`udf/geo/util.rs`) for helpers shared by
   several categories of one provider. Today the guide only mentions category-level `util`.
2. **Backend choice:** add the geo-vs-GEOS policy (question 1) to *Layout*, so the provider of a
   new function follows from a rule.
3. **Signatures:** "A UDF with more than one argument uses `Signature::user_defined` and
   implements `coerce_types` by delegating to `coerce_args`." Add `coerce_types` to the trait
   method order after `signature`. Reason: `Uniform(n, ...)` forces all arguments to one type,
   and `one_of` with exact pairs doesn't scale.
4. **Parameters:** replace "if only scalar values are supported, accept `ColumnarValue::Scalar`
   and return `NotImplemented` for arrays" with "read non-geometry arguments with
   `float_arg`/`boolean_arg`/... and treat a NULL row as a NULL result". Reason: no
   `NotImplemented` paths, and doc examples use per-row parameters.
5. **geo-backed inputs:** "Functions in `udf/geo` iterate rows with `GeometryArg` and match on
   `GeoValue` with explicit `Null`, `Empty` and `Geometry` arms. They don't call
   `downcast_geoarrow_array!` themselves unless they need the original coordinates (Z/M)."
   Reason: one conversion point, and EMPTY behaviour is visible per function.
6. **Imports:** "Import `geo` algorithm traits anonymously (`use geo::Area as _;`)." Reason:
   name clashes with UDF structs.
7. **Families:** "Five or more UDFs that differ only in name, docs and one callback may be
   generated by a `macro_rules!` that expands to the standard anatomy (precedent:
   `make_math_unary_udf!` in datafusion-functions). The macro takes the one-line summary as
   `#[doc]`." Reason: legitimises the predicate macro with a limit.
8. **Return types:** add "If the output's geometry type depends on the input type, return
   `GeometryType`" until a type-generic builder exists.
9. **2D results:** standard documentation wording "Unlike PostGIS, the result is always 2D."
   for `geo`-backed functions that drop Z/M.
10. **Tests:** require column/constant combinations for multi-argument functions in the
    hand-written slt file (§8, first bullet).

## 10. Open questions for the maintainer

1. **geo-vs-GEOS policy. Recommendation:**
   - `geo` is the default backend when its result matches PostGIS under the parity rules (12
     significant digits for numbers and geometries, exact vertex order after a cheap
     deterministic normalization). `geo` is always available, pure Rust and wasm-friendly.
   - When `geo` can't match (different algorithm, different chosen point, message text, or a
     missing algorithm), the function is GEOS-backed (G3) behind the minimum GEOS version
     feature. A native port (G1) is the alternative when PostGIS's own algorithm is small.
   - One backend per SQL function, no runtime switching. Parity runs with `--all-features`, so
     an implementation the suite never exercises would rot.

   Applied, this moves ST_SimplifyPreserveTopology, ST_IsValidReason, ST_PointOnSurface,
   ST_ConcaveHull and ST_DFullyWithin to G3, and keeps ST_ConvexHull/ST_OrientedEnvelope in G2
   with normalization.
2. **Fallbacks for builds without GEOS.** Should the moved functions keep a `geo`
   implementation registered under `#[cfg(not(feature = "geos-..."))]`, documented as "may
   differ from PostGIS"? It helps wasm users (a valid interior point is still useful) but goes
   untested by the parity suite. Recommendation: no, except possibly ST_PointOnSurface.
3. **ST_PointOnSurface:** GEOS (G3) or a native port of JTS `InteriorPointArea`/`Line`/`Point`
   (~200 lines, G1) that gives the same point without GEOS?
4. **Output types:** is `GeometryType` acceptable for simplify and hull outputs, or should G6
   add a type-generic builder (or should we ask geoarrow-rs for one) so typed input keeps its
   type?
5. **Overlapping-polygon GeometryCollections in predicates:** accept and document the
   difference, union polygonal members first (correct but costs a boolean op per row), or route
   to GEOS when it's enabled (violates question 1)?
6. **`geoarrow-expr-geo`:** OK to stop using it (R1)? The alternative is upstreaming PostGIS
   semantics into a crate meant to be engine-neutral.
7. **Priority of Z/M preservation (R8):** Batch 3 as proposed, or earlier?
8. **Geography (G6):** which `Edges` value marks a geography column, and should overload checks
   like "ST_Distance(geometry, geometry, boolean) does not exist" happen at planning? For
   fixed-return-type functions that would mean `return_field_from_args` (it sees field
   metadata), which the style guide reserves for extension return types.
9. **ST_RelateMatch** is text-only. Keep it in G2 because it reuses `geo`'s DE-9IM matcher, or
   move it to G1?
10. **R10:** use `geo::Intersects` for ST_Intersects/ST_Disjoint if a benchmark shows a clear
    win, at the cost of a second code path?
