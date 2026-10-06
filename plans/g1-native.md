# G1: Native functions

Functions implemented from scratch on `geo-traits` and GeoArrow arrays: accessors, constructors,
editors, bounding-box functions, affine transformations, linear referencing, trajectory
functions, and the 3D measurements that `geo` can't do. Existing basis:
`rust/geodatafusion/src/udf/native/{accessors,constructors,bounding_box}`.

The rule for G1 membership: **a function is native when `geo` can't express it without losing
data or changing semantics.** `geo` geometries are 2D, so any function that reads or writes Z or
M, or whose PostGIS behaviour preserves Z/M through a coordinate transform, is native. Functions
that only read structure (counts, types, rings, parts) are native because `geo` adds nothing.

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### 1.1 Final function list

Starting from the inventory's 115 G1 rows, 8 move out and 22 move in, giving **129 functions**
(including the 30 that are registered today).

**Accessors (38):** GeometryType, ST_Boundary, ST_BoundingDiagonal, ST_CoordDim, ST_Dimension,
ST_EndPoint, ST_Envelope, ST_ExteriorRing, ST_GeometryN, ST_GeometryType, ST_HasM, ST_HasZ,
ST_InteriorRingN, ST_IsClosed, ST_IsCollection, ST_IsEmpty, ST_IsPolygonCCW, ST_IsPolygonCW,
ST_M, ST_NDims, ST_NPoints, ST_NRings, ST_NumGeometries, ST_NumInteriorRing(s), ST_NumPoints,
ST_PointN, ST_Points, ST_StartPoint, ST_Summary, ST_X, ST_Y, ST_Z, ST_Zmflag; blocked: ST_CurveN,
ST_HasArc, ST_NumCurves, ST_NumPatches, ST_PatchN.

**Bounding box (9):** ST_3DMakeBox, ST_Expand, ST_MakeBox2D, ST_XMax, ST_XMin, ST_YMax, ST_YMin,
ST_ZMax, ST_ZMin. (`Box2D`/`Box3D` are G6 but share `bounding_box/util/bounds.rs`, so the fixes
in 3.7 apply to them too.)

**Constructors (13):** ST_Hexagon, ST_LineFromMultiPoint, ST_MakeEnvelope, ST_MakePoint,
ST_MakePointM, ST_MakePolygon, ST_Point, ST_PointM, ST_PointZ, ST_PointZM, ST_Polygon, ST_Square,
ST_TileEnvelope; blocked: ST_Letters.

**Editors (34):** ST_AddPoint, ST_CollectionExtract, ST_CollectionHomogenize, ST_FlipCoordinates,
ST_Force2D, ST_Force3D, ST_Force3DM, ST_Force3DZ, ST_Force4D, ST_ForceCollection,
ST_ForcePolygonCCW, ST_ForcePolygonCW, ST_ForceRHR, ST_LineExtend, ST_Multi, ST_Project,
ST_QuantizeCoordinates, ST_RemoveIrrelevantPointsForView, ST_RemovePoint,
ST_RemoveRepeatedPoints, ST_RemoveSmallParts, ST_Reverse, ST_Scroll, ST_Segmentize (geometry),
ST_SetPoint, ST_ShiftLongitude, ST_SnapToGrid, ST_SwapOrdinates; blocked or shim: ST_CurveToLine,
ST_ForceCurve, ST_ForceSFS, ST_LineToCurve.

**Affine transformations (8):** ST_Affine, ST_Rotate, ST_RotateX, ST_RotateY, ST_RotateZ,
ST_Scale, ST_Translate, ST_TransScale.

**Linear referencing (11, from G2):** ST_3DLineInterpolatePoint, ST_AddMeasure, ST_FilterByM,
ST_InterpolatePoint, ST_LineInterpolatePoint, ST_LineInterpolatePoints, ST_LineSubstring,
ST_LocateAlong, ST_LocateBetween, ST_LocateBetweenElevations. (ST_FilterByM is in PostGIS's
processing chapter; it lives with the M functions.)

**Trajectory (4):** ST_ClosestPointOfApproach, ST_CPAWithin, ST_DistanceCPA,
ST_IsValidTrajectory.

**Processing (1, from G2):** ST_ChaikinSmoothing.

**3D measurement (10, from G2):** ST_3DClosestPoint, ST_3DDFullyWithin, ST_3DDistance,
ST_3DDWithin, ST_3DIntersects, ST_3DLength, ST_3DLongestLine, ST_3DMaxDistance, ST_3DPerimeter,
ST_3DShortestLine.

### 1.2 Reassigned out of G1

| Function | To | Why |
|---|---|---|
| ST_IsSimple | G3 | Needs robust self-intersection detection; GEOS `isSimple`. `geo` has no equivalent. |
| ST_IsRing | G3 | `ST_IsClosed AND ST_IsSimple`; follows ST_IsSimple. |
| ST_WrapX | G3 | PostGIS clips polygons at the wrap line with `lwgeom_clip_by_rect` (GEOS). Lines/points alone would be native, but one function should have one mechanism. |
| ST_EstimatedExtent | G6 | Reads table statistics, not a per-row function. Needs catalog/statistics infrastructure, or is declared unsupported. |
| postgis_srs, postgis_srs_all, postgis_srs_codes, postgis_srs_search | G6 | Set-returning queries over the SRS catalogue; depend on the SRID model and a PROJ database. |

### 1.3 Reassigned into G1

| Function | From | Why |
|---|---|---|
| ST_ChaikinSmoothing | G2 | PostGIS smooths Z and M (`LINESTRING(0 0 0,8 8 8,16 0 16)` → `LINESTRING(0 0 0,6 6 6,10 6 10,16 0 16)`); `geo::ChaikinSmoothing` drops them. The algorithm is 20 lines. A commented-out native stub already exists. |
| ST_AddMeasure, ST_FilterByM, ST_InterpolatePoint, ST_LocateAlong, ST_LocateBetween | G2 | Read or write M. `geo` has no M. |
| ST_LocateBetweenElevations, ST_3DLineInterpolatePoint | G2 | Read Z. |
| ST_LineInterpolatePoint(s), ST_LineSubstring | G2 | PostGIS interpolates Z and M at the cut points (`ST_LineInterpolatePoint('LINESTRING(0 0 0 0,2 2 2 2)', 0.5)` → `POINT(1 1 1 1)`); `geo::LineInterpolatePoint` returns 2D. Shares the segment-walking helper with the other LRS functions. |
| ST_3DLength, ST_3DPerimeter | G2 | Z-aware sums of segment lengths; trivial natively. |
| ST_3DDistance, ST_3DMaxDistance, ST_3DClosestPoint, ST_3DShortestLine, ST_3DLongestLine, ST_3DDWithin, ST_3DDFullyWithin, ST_3DIntersects | G2 | `geo` is 2D. They need a native 3D distance kernel (segment–segment, point–plane for polygons), as in PostGIS's `lw_dist3d`. Last batch, L effort. |

ST_LineLocatePoint returns a 2D fraction and can stay in G2 (`geo::LineLocatePoint`); see the open
questions.

### 1.4 Not supported yet

| Function(s) | Reason |
|---|---|
| ST_CurveN, ST_NumCurves, ST_LineToCurve, ST_ForceCurve | GeoArrow has no curve types (CircularString, CompoundCurve, CurvePolygon, MultiCurve, MultiSurface). The WKT parser rejects them (`WKT error: Invalid type encountered`). |
| ST_NumPatches, ST_PatchN | PolyhedralSurface and TIN aren't representable. |
| ST_HasArc, ST_CurveToLine, ST_ForceSFS | Implementable as shims: always `false` / identity, because no curves can reach them. They unlock no doc tests (every example uses curves). Open question. |
| ST_Letters | Needs PostGIS's embedded font geometries, which are GPL-2.0 data. Porting them would put GPL data in an MIT/Apache crate. |
| ST_MemSize | PostgreSQL varlena size. There's no meaningful equivalent for an Arrow value. |
| Nested GeometryCollections | GeoArrow forbids them, so e.g. `ST_CollectionHomogenize('GEOMETRYCOLLECTION(POINT(0 0), GEOMETRYCOLLECTION(LINESTRING(1 1, 2 2)))')` can't pass. |

Doc tests that use curves, TINs or polyhedral surfaces are permanently out of reach. This caps
GeometryType, ST_GeometryType, ST_CoordDim, ST_StartPoint, ST_EndPoint, ST_IsEmpty, ST_IsClosed,
ST_GeometryN, ST_Zmflag, ST_Force*, ST_Translate, ST_TransScale and ST_PointN below 100%.

### 1.5 Corrections to the inventory and README

- **ST_Envelope**, **ST_Expand** and **ST_PointN** are marked implemented but aren't registered.
  `accessors/envelope.rs` and `bounding_box/expand.rs` aren't in their `mod.rs` and use the
  removed `geoarrow` 0.3 API. `PointN` in `accessors/line_string.rs:524-577` is
  `#[expect(dead_code)]`. G1 has 30 working functions, not 33. README.md lines 56 and 359 are
  wrong too.
- **ST_ConcaveHull** (G2) is marked implemented, but only the commented-out
  `native/processing/concave_hull.rs` mentions it. Since PostGIS 3.3 it wraps GEOS
  `GEOSConcaveHull` (`param_pctconvex`), so it probably belongs in G3. Flagged for G2/G3.
- **ST_NumPoints** is registered as an alias of ST_NPoints (`npoints.rs:368`), but PostGIS
  defines it for LineStrings only: `ST_NumPoints('MULTIPOINT(0 0)')` is NULL. It needs its own
  UDF.

## 2. Existing basis

### 2.1 How the current functions work

Every scalar UDF has the shape the style guide describes: a struct with `new()` + `Default`, a
`ScalarUDFImpl` with `name`/`signature`/`return_type`/`invoke_with_args`/`documentation`, and a
free `*_impl(args)` function that:

1. converts arguments with `ColumnarValue::values_to_arrays`,
2. decodes the geometry column (`from_arrow_array` or `GeoArrowType::from_arrow_field` +
   `wrap_array`),
3. dispatches with `downcast_geoarrow_array!` to a generic per-array function over
   `&'a impl GeoArrowArrayAccessor<'a>`,
4. loops over `array.iter()`, matches `geom.as_type()` and appends to an Arrow builder,
   `append_null()` for NULL input.

Geometry outputs use GeoArrow builders (`PointBuilder`, `GeometryBuilder`, `RectBuilder`) and
compute the output field in `return_field_from_args` (`line_string.rs:579-598`, `box.rs:119-123`).
Owned geometries with Z/M are built from `wkt::types` (`line_string.rs:708-716`,
`constructors/point.rs:567-618`, `bounds.rs:207-233`), because `geo-traits` 0.3 has no owned types
and `geo` types are 2D.

### 2.2 Bugs

| File:line | Bug | PostGIS |
|---|---|---|
| `accessors/point.rs:139-159` | `ST_Z` uses `coord.nth(2)`, which is M for an XYM point. | `ST_Z('POINTM(1 2 3)')` is NULL. |
| `accessors/point.rs:52-54` (and 107, 195, 250) | Return field is declared non-nullable, but the function returns NULLs. | — |
| `accessors/point.rs:150-152` | Non-point input returns NULL. | Error: `Argument to ST_X() must have type POINT`. |
| `accessors/point.rs:26` | `any_point_type_input(1)` accepts only native Point and Geometry arrays. WKB/WKT input fails to plan, unlike every other function. | — |
| `accessors/line_string.rs:694` | `geom.num_coords() - 1` underflows for `LINESTRING EMPTY`. | NULL. |
| `accessors/line_string.rs:695-702` | `Mode::N`: `n < num_coords` excludes the last point, `n = 0` underflows, and negative indices aren't supported. | `ST_PointN(line, -1)` is the last point; 0 and out of range are NULL. |
| `accessors/line_string.rs:708-716` | `coord_to_point` uses `nth(2)`/`nth(3)`, so XYM becomes XYZ for Geometry-array input. | M stays M. |
| `accessors/line_string.rs:680-686` | `ST_StartPoint` returns NULL for anything but LINESTRING. | `ST_StartPoint` returns the first coordinate of POINT, MULTIPOINT, POLYGON and MULTILINESTRING. `ST_EndPoint` is NULL for those. Verified on 3.6.4. |
| `accessors/geometry_type.rs:210-219` | `GeometryType` appends `Z`/`ZM`. | Only XYM gets a suffix: `GeometryType('POINT Z(1 2 3)')` = `POINT`, `GeometryType('POINTM(1 2 3)')` = `POINTM`. |
| `accessors/geometry_type.rs:201` | `"ST_MultilineString"` | `ST_MultiLineString` |
| `accessors/coord_dim.rs:371-379` | Native arrays return one `ColumnarValue::Scalar`, so NULL rows become 2/3/4. | NULL in, NULL out. |
| `accessors/is_closed.rs:116-117` | Compares only X and Y. | Compares Z: `ST_IsClosed('LINESTRING(0 0 0,1 1 1,0 0 1)')` = false. |
| `accessors/is_closed.rs:98` | GeometryCollection → false. | `ST_IsClosed('GEOMETRYCOLLECTION(POINT(1 1))')` = true. Needs a hand-written check per element type. |
| `accessors/npoints.rs:368` | `st_numpoints` alias (see 1.5). | — |
| `bounding_box/util/bounds.rs:38-50, 335-343` | EMPTY input produces a ±∞ rect instead of NULL. | `Box2D('POINT EMPTY')` and `ST_XMin('POINT EMPTY')` are NULL. |
| `bounding_box/util/bounds.rs:87` | `add_coord` uses `nth(2)`, so M is treated as Z. | `ST_ZMax('POINTM(1 2 5)')` = 0. |
| `bounding_box/extrema.rs:96, 316` | ZMin/ZMax of 2D input return `f64::MIN`/`f64::MAX`. | 0. |
| `bounding_box/make_box.rs:252, 318, 370` | Output CRS is always default. | Propagates the input SRID. |
| `constructors/point.rs:639` | The separated-coordinates path takes the null buffer from `x` only. `ST_Point(1, NULL)` is a valid point. | NULL. |
| `constructors/point.rs:579-593` | `ST_MakePointM` with `CoordType::Interleaved` puts M into `z`. `PointBuilder::push_coord` then panics on the XYZ/XYM dimension mismatch. Not covered by tests because the default is Separated. | — |
| `constructors/point.rs:75` | `srid_val.unwrap()` panics for `ST_Point(1, 2, NULL)`. | — |

### 2.3 Inconsistencies between the existing functions

- **Geometry decoding:** `from_arrow_array` (npoints, geometry_type, point, line_string, extrema,
  box) vs `GeoArrowType::from_arrow_field` + `wrap_array` (is_empty.rs:108-109,
  is_closed.rs:70-71). They do the same thing.
- **Argument extraction:** `.into_iter().next().unwrap()` (is_empty.rs:104-107,
  coord_dim.rs:338-341, point.rs:129-132, is_closed.rs:66-69), `arrays[0]` (npoints.rs:406,
  line_string.rs:614), `.expect(..)` (dump.rs:122), `arrays.next().unwrap()` (make_box.rs:343).
- **Per-array function naming:** `impl_is_empty`, `impl_is_closed` (style-conforming) vs
  `_num_points_impl`, `_geometry_type_impl`, `_nth_impl`, `_m_impl`, `geometry_impl`,
  `polygon_impl`, `impl_fixed_dim`, `impl_variable_dim`, `impl_array_accessor`.
- **Error type of the loop:** `GeoArrowResult` (npoints, geometry_type, num_interior_rings,
  bounds) vs `GeoDataFusionResult` (is_empty, is_closed, point, line_string).
- **Implementation fn naming:** NPoints delegates to `coord_dim_impl` (npoints.rs:388, 404), a
  copy-paste leftover.
- **Signatures:** shared static helper (most) vs a `signature` field built in `new()` (X/Y/Z/M,
  all six point constructors) vs file-level `LazyLock` (make_box.rs:223, 289).
- **Fixed return types:** `return_type` (most) vs `return_field_from_args` with a hard-coded
  `Field` (X/Y/Z/M).
- **Integer and string types:** UInt8 (CoordDim, NDims), UInt32 (NPoints, NumInteriorRings),
  Utf8View (GeometryType). PostGIS returns `smallint`/`integer`/`text`. Unsigned results make
  `ST_NPoints(g) - 1` overflow on empty input.
- **Specialised fast paths:** NumInteriorRings (`num_interior_rings.rs:258-261`) and
  StartPoint/EndPoint (`line_string.rs:617-629`) special-case one native type; nothing else does.
  The NumInteriorRings fast path is redundant with the generic loop.
- **Documentation statics:** `DOCUMENTATION`, `ST_DOCUMENTATION`, `NDIMS_DOCUMENTATION`,
  `XMIN_DOC`, `DOC_2D`, `DOC_3D`, `POINT_DOC`, `MAKE_POINT_M_DOC`, `START_POINT_DOCUMENTATION`.
- **Documentation content:** NumInteriorRings repeats NPoints' description and syntax
  (num_interior_rings.rs:241-249). Argument names are `g1`, `geom`, `a_point` or `box`, with
  descriptions instead of SQL types in extrema.rs (`"The geometry or box input"`) and point.rs
  (`"x value"`). Extrema UDFs list themselves in `with_related_udf`. `with_related_udf`
  mixes case (`"ST_MakePointM"`, point.rs:474).
- **Struct doc comments:** only `Dump` has one.
- **Trait method order:** `aliases` comes after `signature` (npoints.rs:375-381,
  num_interior_rings.rs:224-230).
- **Logic in trait methods:** the point constructors do the work in `invoke_with_args` and
  `return_field_from_args` (point.rs:69-91) instead of delegating to free functions.
- **Error variants:** `Internal` for a user error (point.rs:78); `unreachable!()` for
  input-dependent states (make_box.rs:360, 364; bounds.rs:166, 324; point.rs:448).
- **Python bindings:** `IsEmpty` has none. The `.pyi` stubs for `StartPoint`/`EndPoint` omit
  the `coord_type` keyword.
- **Unit tests:** behaviour tests that loop over cases with `unwrap_or_else(panic!)`
  (is_empty.rs, is_closed.rs) vs single-assert tests. Most behaviour belongs in slt now.

### 2.4 `udf/native/processing/`

It's commented out (`native/mod.rs:7`) and written against the pre-0.4 `geoarrow` API
(`geoarrow::algorithm::geo::*`, `parse_to_native_array`, `GEOMETRY_TYPE`). Nothing in it can be
revived. `chaikin_smoothing.rs` even registers as `st_convexhull`. **Delete the directory.**
ST_ChaikinSmoothing is reimplemented natively in a new `native/processing/` (see 1.3).
ST_ConcaveHull goes to G2/G3.

## 3. Refactoring assessment

### R1. A shared per-geometry kernel (do)

Every function repeats the same loop: iterate, propagate the scalar's error, append NULL for
NULL input, append the value otherwise. A trait with a generic method plus two drivers moves
NULL handling and error propagation into one place. This mirrors arrow's `compute::unary`, but
over geometries. Closures can't be generic over `impl GeometryTrait`, so it has to be a trait.
The sketch below compiles against geoarrow-array 0.8.0 / arrow-array 58 (checked in a scratch
crate).

Before (`is_empty.rs:103-141`, 39 lines):

```rust
fn is_empty_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let array = ColumnarValue::values_to_arrays(&args.args)?
        .into_iter()
        .next()
        .unwrap();
    let geo_type = GeoArrowType::from_arrow_field(&args.arg_fields[0])?;
    let geo_array = geo_type.wrap_array(&array)?;
    let geo_array_ref = geo_array.as_ref();
    let result = downcast_geoarrow_array!(geo_array_ref, impl_is_empty)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

fn impl_is_empty<'a>(array: &'a impl GeoArrowArrayAccessor<'a>) -> GeoDataFusionResult<BooleanArray> {
    let mut builder = BooleanBuilder::with_capacity(array.len());
    for item in array.iter() {
        match item {
            Some(geom) => builder.append_value(is_geometry_topologically_empty(&geom?)),
            None => builder.append_null(),
        }
    }
    Ok(builder.finish())
}
```

After:

```rust
fn is_empty_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let [geom] = take_function_args("ST_IsEmpty", ColumnarValue::values_to_arrays(&args.args)?)?;
    let geo_array = from_arrow_array(&geom, &args.arg_fields[0])?;
    let result: BooleanArray = map_geometry(geo_array.as_ref(), &IsEmptyKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsEmptyKernel;

impl GeometryKernel for IsEmptyKernel {
    type Output = bool;

    fn eval(&self, geom: &impl GeometryTrait<T = f64>, _row: usize) -> GeoDataFusionResult<Option<bool>> {
        Ok(Some(is_geometry_topologically_empty(geom)))
    }
}
```

The `row` argument lets a kernel read the other arguments of the same row from arrays it holds
(`self.dx.value(row)`), which covers every multi-argument function in the group. The kernel
returns `Ok(None)` for SQL NULL output (non-point input to ST_PointN, NULL `dx`).

Effort M (helper plus migrating 15 files). Risk low: the helper is internal, the
`downcast_geoarrow_array!` dispatch is unchanged, and the slt suite covers behaviour.

### R2. One output-builder enum and an output-type rule (do)

`geoarrow-array`'s `GeoArrowArrayBuilder` trait is `pub(crate)` and not dyn-compatible, so a
function that returns "the same type as its input" can't pick a builder at runtime. Today only
StartPoint/EndPoint try, with two hand-written code paths (`line_string.rs:617-674`). A small
enum over the native builders, plus a rule for the output type, gives every geometry-returning
function the same two lines:

```rust
let output_type = same_type_output(geo_array.data_type(), coord_type);
let result = map_geometry_to_geoarrow(geo_array.as_ref(), &TranslateKernel { .. }, output_type)?;
```

Before: `impl_fixed_dim` + `impl_variable_dim` + `return_field_impl` with a LineString branch
(80 lines). After: a kernel plus one call. Effort S. Risk low. `PointBuilder::push_coord` and
`push_point` panic on a dimension mismatch, so the enum uses the `push_geometry` methods, which
return `GeoArrowResult`.

### R3. Named ordinate access and one owned geometry type (do)

The M-as-Z bugs (2.2) all come from `CoordTrait::nth(2)`, which is Z for XYZ and M for XYM. Add
`z(coord)`/`m(coord)` helpers that go through `dim()`, and one conversion from any
`GeometryTrait` to an owned `wkt::Wkt<f64>` that applies a coordinate function. `wkt` 0.14 is
already a dependency, its types implement `geo-traits` with the dimension derived from
`z`/`m: Option<f64>`, and they're what the existing code already uses ad hoc. Every editor and
transformation in the group then becomes "map coordinates" or "rebuild structure" on one type.

```rust
// Before (line_string.rs:708-716): M ends up in z for XYM input.
let coord = wkt::types::Coord { x: coord.x(), y: coord.y(), z: coord.nth(2), m: coord.nth(3) };

// After
let coord = to_owned_coord(&coord); // Coord { x, y, z: z(&c), m: m(&c) }
```

Effort S. Risk low. A zero-copy alternative (lazy `geo-traits` wrapper types that map
coordinates on access) avoids the allocation per geometry but needs about 400 lines of GAT
plumbing. Later, if profiling shows it matters.

### R4. Signatures: statics only, every encoding, PostGIS parameter names (do)

- Remove the `signature` field from X/Y/Z/M and the point constructors; use `static SIGNATURE`
  or `data_types.rs` helpers, as the style guide already requires.
- X/Y/Z/M accept every geometry encoding (`any_single_geometry_type_input()`) and raise an
  `Execution` error for non-points, as PostGIS does.
- Add `geometry_and(overloads)` to `data_types.rs` for `(geometry, <scalars>...)` functions:
  a `OneOf` of `TypeSignature::Exact` for every geometry type × overload. DataFusion coerces
  integer literals to `Float64` for `Exact` (`type_coercion/functions.rs:972-999`), so
  `ST_Translate(g, 1, 2)` plans.
- Attach parameter names with `Signature::with_parameter_names` exactly where PostGIS declares
  them (check `pg_get_function_arguments`: `st_point(float8, float8, srid integer)` has one,
  `st_translate` has none). Doc tests use `ST_PointZ(..., srid => 4326)`. DataFusion needs a
  name for every parameter, so where PostGIS names only some (`st_point`), use the reference
  docs' names for the rest. PostGIS `DEFAULT` parameters become a shorter `Exact` overload. For `OneOf`, names must
  match the longest overload (`signature.rs:1457-1492`), which fits PostGIS's
  trailing-optional-argument style.

```rust
// Before (constructors/point.rs:29-45): built per instance, no names.
signature: Signature::one_of(vec![TypeSignature::Exact(vec![Float64, Float64, Float64]), ...], ..)

// After
static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::one_of(
        vec![
            TypeSignature::Exact(vec![DataType::Float64; 3]),
            TypeSignature::Exact(vec![DataType::Float64, DataType::Float64, DataType::Float64, DataType::Int64]),
        ],
        Volatility::Immutable,
    )
    // The names PostGIS declares: st_pointz(xcoordinate, ycoordinate, zcoordinate, srid DEFAULT 0).
    .with_parameter_names(vec!["xcoordinate", "ycoordinate", "zcoordinate", "srid"])
    .expect("four names for a four-argument signature")
});
```

Effort S. Risk low. Coordinate with G6, which owns `data_types.rs`.

### R5. PostGIS return types (do)

Map PostGIS SQL types one-to-one: `integer` → Int32, `smallint` → Int16, `float8` → Float64,
`boolean` → Boolean, `text` → Utf8. Changes CoordDim/NDims (UInt8 → Int16), NPoints and
NumInteriorRings (UInt32 → Int32), and GeometryType/ST_GeometryType (Utf8View → Utf8). It's a
breaking Rust/Python API change, but avoids unsigned arithmetic surprises and matches what users
cast to. Effort S. Risk: downstream users matching on UInt types. The slt renderer treats all
integers alike, so parity is unaffected.

### R6. Argument extraction and error variants (do)

Use `datafusion::common::utils::take_function_args` (destructures into `[ArrayRef; N]` with a
proper error) instead of `next().unwrap()`/indexing. Replace `unreachable!()` on input-dependent
paths with `Internal` errors, `Internal` for user errors with `Plan`/`Execution`, and the SRID
`unwrap()` with NULL handling. Effort S. Risk none.

### R7. Fix the bounds utility (do)

`BoundingRect` gets `Option`-style emptiness (`is_empty()` when nothing was added), so EMPTY
input yields NULL. Z is collected with `z()` (R3), never from M. `include_z` boxes without Z
report Z = 0, which is what PostGIS's `Box3D` and `ST_ZMin`/`ST_ZMax` do. `Triangle`/`Line` are
handled (their coordinates are added) instead of `unreachable!()`. Drop the `assert_eq!` in
`Add`. ZMin/ZMax closures become `rect.min().nth(2).unwrap_or(0.0)`. Effort S. Risk low, and it
also fixes `Box2D`/`Box3D`/`ST_Extent` (G5/G6).

### R8. Delete dead code (do)

Delete `native/processing/`, `accessors/envelope.rs` and `bounding_box/expand.rs`, and the
commented-out `register_native` in `native/mod.rs`. ST_Envelope and ST_Expand are rewritten
from the template in batch 1. The `PointN` stub in `line_string.rs` is replaced by a real UDF.
Effort S. Risk none.

### R9. Always broadcast scalar arguments (do)

Non-geometry arguments go through `values_to_arrays` and are read per row, so `ST_Translate(g,
col_dx, 1)` works. Only arguments that change the *return type* (SRID, which sets the output
CRS) stay scalar-only, read from `ReturnFieldArgs::scalar_arguments` with a `Plan` error
otherwise. The cost (a broadcast array per scalar) is negligible next to geometry work. Effort
none for new code; ST_Point/ST_PointZ/... already do this for coordinates.

### R10. Python bindings and documentation clean-up (do)

Add `PyIsEmpty`; fix the StartPoint/EndPoint stubs; fix the documentation listed in 2.3
(descriptions, PostGIS argument names and SQL types, related UDFs, `DOCUMENTATION` statics).
Effort S. Risk none.

### R11. Coordinate-buffer fast path for affine transforms (later)

For native arrays (not WKB/WKT), affine transforms, `ST_FlipCoordinates`, `ST_SwapOrdinates`
and `ST_ShiftLongitude` could map the coordinate buffer once and reuse offsets and validity, with
no per-geometry allocation. It's a real speed-up but needs per-type array reconstruction
(`LineStringArray::new(coords, offsets, nulls, metadata)` etc., and every child of
`GeometryArray`). Do it after the group is complete and only with a benchmark.

### R12. One struct per extremum family (don't)

XMin…ZMax are six near-identical structs (`extrema.rs`, 392 lines). Collapsing them into one
parameterised struct would change the public Rust/Python API, and a macro-generated
`ScalarUDFImpl` would hide the shape that every file is supposed to share. DataFusion itself
doesn't macro-generate UDF impls (only the `make_udf_function!` singleton getters). Keep them.

### R13. `#[user_doc]` for documentation (don't, for now)

`datafusion-functions` uses the `datafusion_macros::user_doc` attribute instead of
`Documentation::builder`. `datafusion` doesn't re-export it, so it'd be a new dependency for
the whole crate. It's a G6 decision, not a G1 one.

## 4. Canonical template

### 4.1 Shared helpers to add

They're useful to G2 too, so they live crate-wide in `rust/geodatafusion/src/util/` (crate
private), next to `data_types.rs` and `error.rs`. G6 owns scaffolding; G1 is the first user.

| Module | Item | Signature |
|---|---|---|
| `util/kernel.rs` | `GeometryKernel` | `pub(crate) trait GeometryKernel { type Output; fn eval(&self, geom: &impl GeometryTrait<T = f64>, row: usize) -> GeoDataFusionResult<Option<Self::Output>>; }` |
| | `map_geometry` | `pub(crate) fn map_geometry<O, K>(array: &dyn GeoArrowArray, kernel: &K) -> GeoDataFusionResult<O> where K: GeometryKernel, O: FromIterator<Option<K::Output>>` (BooleanArray, PrimitiveArray, StringArray) |
| | `map_geometry_to_geoarrow` | `pub(crate) fn map_geometry_to_geoarrow<K>(array: &dyn GeoArrowArray, kernel: &K, output_type: GeoArrowType) -> GeoDataFusionResult<Arc<dyn GeoArrowArray>> where K: GeometryKernel, K::Output: GeometryTrait<T = f64>` |
| `util/builder.rs` | `GeoArrowBuilder` | enum over `PointBuilder` … `GeometryCollectionBuilder`, `GeometryBuilder`; `new(GeoArrowType) -> GeoDataFusionResult<Self>`, `push_geometry(Option<&impl GeometryTrait<T = f64>>)`, `finish() -> Arc<dyn GeoArrowArray>` |
| | `same_type_output` | `pub(crate) fn same_type_output(input: &GeoArrowType, coord_type: CoordType) -> GeoArrowType`: native types keep type and dimension; Rect → Polygon; WKB/WKT → Geometry; metadata kept |
| | `input_metadata` | `pub(crate) fn input_metadata(field: &Field) -> Arc<Metadata>`: `Arc::new(Metadata::try_from(field).unwrap_or_default())`, replacing six copies |
| `util/ordinates.rs` | `z`, `m` | `pub(crate) fn z(coord: &impl CoordTrait<T = f64>) -> Option<f64>`, likewise `m` |
| | `to_owned_coord` | `pub(crate) fn to_owned_coord(coord: &impl CoordTrait<T = f64>) -> wkt::types::Coord<f64>` |
| `util/owned.rs` | `map_coords` | `pub(crate) fn map_coords(geom: &impl GeometryTrait<T = f64>, f: &impl Fn(wkt::types::Coord<f64>) -> wkt::types::Coord<f64>) -> GeoDataFusionResult<wkt::Wkt<f64>>`: structure-preserving copy; output dimension follows `f`'s coordinates; empty parts keep the input dimension |
| | `to_owned_geometry` | `map_coords(geom, &|c| c)` |
| `data_types.rs` | `geometry_and` | `pub(crate) fn geometry_and(overloads: &[&[DataType]]) -> Signature` (R4) |
| `native/linear_referencing/util.rs` | segment walker | `fn locate_along_segments(...)`: shared by ST_LineInterpolatePoint(s), ST_LineSubstring, ST_LocateAlong/Between, ST_AddMeasure, ST_InterpolatePoint; interpolates Z and M |
| `native/affine_transformations/util.rs` | `Affine3D` | `struct Affine3D { a, b, c, d, e, f, g, h, i, xoff, yoff, zoff }` + `fn apply(&self, Coord) -> Coord` (Z only if present; M untouched). Every affine UDF builds one and calls `map_coords` |

New categories, named after the PostGIS chapters: `native/editors`, `native/affine_transformations`,
`native/linear_referencing`, `native/trajectory`, `native/processing`, `native/measurement`
(3D functions).

### 4.2 Fixed Arrow return type (`accessors/num_geometries.rs`)

```rust
use std::sync::{Arc, OnceLock};

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::common::utils::take_function_args;
use datafusion::error::Result;
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, MultiLineStringTrait, MultiPointTrait,
    MultiPolygonTrait,
};
use geoarrow_array::array::from_arrow_array;

use crate::data_types::any_single_geometry_type_input;
use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::kernel::{GeometryKernel, map_geometry};

/// Returns the number of elements in a geometry collection.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NumGeometries;

impl NumGeometries {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NumGeometries {
    fn default() -> Self {
        Self::new()
    }
}

static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl ScalarUDFImpl for NumGeometries {
    fn name(&self) -> &str {
        "st_numgeometries"
    }

    fn signature(&self) -> &Signature {
        any_single_geometry_type_input()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(num_geometries_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns the number of elements in a geometry collection. \
                 Single geometries return 1 and empty geometries return 0.",
                "ST_NumGeometries(geom)",
            )
            .with_argument("geom", "geometry")
            .with_related_udf("st_geometryn")
            .build()
        }))
    }
}

fn num_geometries_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let [geom] = take_function_args("ST_NumGeometries", ColumnarValue::values_to_arrays(&args.args)?)?;
    let geo_array = from_arrow_array(&geom, &args.arg_fields[0])?;
    let result: Int32Array = map_geometry(geo_array.as_ref(), &NumGeometriesKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct NumGeometriesKernel;

impl GeometryKernel for NumGeometriesKernel {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        use geo_traits::GeometryType::*;

        // PostGIS returns 0 for a topologically empty geometry, but otherwise counts
        // members structurally: `MULTIPOINT(EMPTY, (1 1))` has 2 and
        // `GEOMETRYCOLLECTION(POINT EMPTY)` has 0.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(0));
        }
        let n = match geom.as_type() {
            MultiPoint(g) => g.num_points(),
            MultiLineString(g) => g.num_line_strings(),
            MultiPolygon(g) => g.num_polygons(),
            GeometryCollection(g) => g.num_geometries(),
            Point(_) | LineString(_) | Polygon(_) | Rect(_) | Triangle(_) | Line(_) => 1,
        };
        Ok(Some(
            i32::try_from(n).expect("GeoArrow offsets are i32, so member counts fit in i32"),
        ))
    }
}
```

### 4.3 Geometry return type with per-row arguments (`affine_transformations/translate.rs`)

```rust
use std::sync::{Arc, LazyLock, OnceLock};

use arrow_array::cast::AsArray;
use arrow_array::{Array, Float64Array};
use arrow_array::types::Float64Type;
use arrow_schema::{DataType, FieldRef};
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::from_arrow_array;
use geoarrow_schema::{CoordType, GeoArrowType};

use crate::data_types::geometry_and;
use crate::error::GeoDataFusionResult;
use crate::udf::native::affine_transformations::util::Affine3D;
use crate::util::builder::same_type_output;
use crate::util::kernel::{GeometryKernel, map_geometry_to_geoarrow};
use crate::util::owned::map_coords;

/// Translates a geometry by given offsets.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Translate {
    coord_type: CoordType,
}

impl Translate {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for Translate {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

// PostGIS declares ST_Translate's parameters without names, so there are no
// `with_parameter_names` here.
static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| geometry_and(&[&[DataType::Float64; 2], &[DataType::Float64; 3]]));
static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl ScalarUDFImpl for Translate {
    fn name(&self) -> &str {
        "st_translate"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("return_type".to_string()))
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(return_field_impl(args, self.coord_type)?)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(translate_impl(args, self.coord_type)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns a new geometry whose coordinates are translated by deltax, deltay and \
                 deltaz. M values are unchanged.",
                "ST_Translate(g1, deltax, deltay, deltaz)",
            )
            .with_argument("g1", "geometry")
            .with_argument("deltax", "float")
            .with_argument("deltay", "float")
            .with_argument("deltaz", "float")
            .with_related_udf("st_affine")
            .build()
        }))
    }
}

fn return_field_impl(args: ReturnFieldArgs, coord_type: CoordType) -> GeoDataFusionResult<FieldRef> {
    let input_type = GeoArrowType::from_arrow_field(args.arg_fields[0].as_ref())?;
    Ok(Arc::new(same_type_output(&input_type, coord_type).to_field("", true)))
}

fn translate_impl(args: ScalarFunctionArgs, coord_type: CoordType) -> GeoDataFusionResult<ColumnarValue> {
    let arrays = ColumnarValue::values_to_arrays(&args.args)?;
    let geo_array = from_arrow_array(&arrays[0], &args.arg_fields[0])?;
    let kernel = TranslateKernel {
        dx: arrays[1].as_primitive::<Float64Type>(),
        dy: arrays[2].as_primitive::<Float64Type>(),
        dz: arrays.get(3).map(|a| a.as_primitive::<Float64Type>()),
    };
    let output_type = same_type_output(geo_array.data_type(), coord_type);
    let result = map_geometry_to_geoarrow(geo_array.as_ref(), &kernel, output_type)?;
    Ok(ColumnarValue::Array(result.into_array_ref()))
}

struct TranslateKernel<'a> {
    dx: &'a Float64Array,
    dy: &'a Float64Array,
    dz: Option<&'a Float64Array>,
}

impl GeometryKernel for TranslateKernel<'_> {
    type Output = wkt::Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<wkt::Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        if self.dx.is_null(row) || self.dy.is_null(row) || self.dz.is_some_and(|dz| dz.is_null(row)) {
            return Ok(None);
        }
        // PostGIS implements ST_Translate as ST_Affine(g, 1, 0, 0, 0, 1, 0, 0, 0, 1, dx, dy, dz).
        // Using the same matrix keeps floating-point results identical.
        let affine = Affine3D::translate(
            self.dx.value(row),
            self.dy.value(row),
            self.dz.map_or(0.0, |dz| dz.value(row)),
        );
        Ok(Some(map_coords(geom, &|c| affine.apply(c))?))
    }
}
```

The `arrays[i]` indexing is fine because the signature fixes the arity; with an optional
argument it's `arrays.get(i)`. (`take_function_args` only fits a fixed arity.)

### 4.4 Variations

- **No geometry input** (constructors: ST_MakeEnvelope, ST_TileEnvelope, ST_Hexagon): no kernel;
  zip the numeric arrays and push into a builder of the guaranteed type (`PolygonType`, XY,
  `coord_type`), as `create_point_array` does.
- **Guaranteed output type** (ST_StartPoint, ST_PointN, ST_Points, ST_Envelope): the output type
  is fixed (`PointType` with the input dimension when the input has one, `GeometryType`
  otherwise), not `same_type_output`.
- **Dimension-changing editors** (ST_Force*): `same_type_output(..).with_dimension(dim)`;
  for Geometry input the builder takes the dimension from each output geometry.
- **SRID arguments**: read from `ReturnFieldArgs::scalar_arguments` with the G6 SRID helper;
  arrays are a `Plan` error ("ST_Point only supports SRID as a scalar integer").
- **Box inputs** (ST_Expand(box2d)): dispatch on `GeoArrowType::Rect` in both
  `return_field_from_args` and the impl, and use `RectBuilder` with the input's `BoxType`.

## 5. Dependencies

### 5.1 Crate APIs (verified in `~/.cargo/registry`, versions from `Cargo.lock`)

| Crate | Version | APIs relied on |
|---|---|---|
| geoarrow-array | 0.8.0 | `downcast_geoarrow_array!` (`cast.rs:833`, extra args by value); `GeoArrowArrayAccessor::iter`; `array::from_arrow_array`; `GeoArrowArray::{data_type, into_array_ref, to_array_ref}`; builders `PointBuilder::{with_capacity, push_geometry}`, `LineStringBuilder`, `PolygonBuilder`, `MultiPointBuilder`, `MultiLineStringBuilder`, `MultiPolygonBuilder`, `GeometryCollectionBuilder` (`new`, `push_geometry`, `finish`), `GeometryBuilder::{new, push_geometry, push_null, finish}` (per-row dimension from `geom.dim()`), `RectBuilder::{with_capacity, push_rect, push_null}`; `cast::AsGeoArrowArray` |
| geoarrow-schema | 0.8.0 | `GeoArrowType::{from_arrow_field, with_coord_type, with_dimension, with_metadata, metadata, dimension}` (`datatype.rs:221-270`); `Dimension: TryFrom<geo_traits::Dimensions>` (`dimension.rs:97`); `Metadata: TryFrom<&Field>`; `Crs::from_authority_code`; `BoxType`, `PointType`, `PolygonType`, `GeometryType` |
| geo-traits | 0.3.0 | `GeometryTrait::{as_type, dim}`, `CoordTrait::{x, y, nth, nth_or_panic}`, `Dimensions`; no owned geometry types (hence `wkt`) |
| wkt | 0.14.0 | `types::{Coord { x, y, z, m }, Point::{new, empty, from_coord}, LineString::new, Polygon::new, MultiPoint::new, MultiLineString::new, MultiPolygon::new, GeometryCollection::new, Dimension}`, `Wkt` enum; `CoordTrait::dim` derived from `z`/`m` (`types/coord.rs:31-41, 82-90`) |
| datafusion | 54.0.0 | `ScalarUDFImpl::{return_field_from_args, coerce_types}`; `ScalarFunctionArgs { args, arg_fields, number_rows, return_field, .. }`; `ReturnFieldArgs { arg_fields, scalar_arguments }`; `Signature::{one_of, exact, uniform, with_parameter_names}`; `TypeSignature::Exact` coerces numerics (`coerced_from`); `common::utils::take_function_args` |
| arrow-array | 58.3 | `BooleanArray`, `PrimitiveArray<T>`, `StringArray`: `FromIterator<Option<_>>` (used by `map_geometry`) |
| geo | 0.31.0 | Not needed by G1. `AffineTransform` is a 2D 3×3 matrix (`affine_ops.rs:124`) and can't express ST_Affine's 12 parameters or Z. |
| std | MSRV 1.88 | `f64::round_ties_even` (stable 1.77) for ST_SnapToGrid, which PostGIS implements with C `rint` (ties to even). |

### 5.2 Why affine transformations are G1, not G2

`geo::AffineOps` is 2D. PostGIS's ST_Affine takes a 3×3 matrix plus a 3D offset and transforms
Z (`ST_Affine('POINT(1 2 3)', 1,0,0,0,1,0,0,0,1,1,1,1)` → `POINT(2 3 4)`). Every 2D affine
function preserves Z and M (`ST_Translate('POINTM(1 2 3)', 1, 1)` → `POINTM(2 3 3)`), and
ST_Scale with a `POINT ZM` factor scales M too. Going through `geo` would drop Z/M, so it'd need
a native path anyway. The native implementation is a 12-number matrix applied in `map_coords`.

### 5.3 Dependencies on other groups

| Group | Needed | For |
|---|---|---|
| G4 | `ST_GeomFromEWKT`, `ST_AsEWKT` | About 45 G1 doc-test records use them (st_affine, st_rotate, st_scale, st_force*, st_x/y/z/m, st_ndims, st_zmflag, st_geometryn, …). The biggest single unlock for G1 parity. |
| G4 | `ST_AsText` formatting (`MULTIPOINT((0 0),(1 1))`, 15 significant digits) | Most G1 doc tests wrap results in ST_AsText. |
| G6 | SRID model: SRID ↔ `Crs` helper, `ST_SetSRID`, `ST_SRID` | ST_Point/ST_PointZ/… `srid`, ST_MakeEnvelope, ST_Polygon, ST_TileEnvelope (3857 default bounds), ST_Hexagon/ST_Square (origin SRID); SRID propagation tests. |
| G6 | `geometry` type and casts; `box2d`/`box3d` types | ST_Expand(box2d), ST_RemoveIrrelevantPointsForView(bounds box2d). |
| G6 | `geometry[]` (List of a GeoArrow extension type) | ST_MakePolygon(shell, holes[]). |
| G6 / upstream | geoarrow-rs fixes: `GeometryBuilder::push_geometry` turns a one-element GeometryCollection into its element (`builder/geometry.rs:641-646`); `MultiPolygonBuilder` panics on `MULTIPOLYGON(EMPTY, ...)` (`is_empty.rs:292`) | ST_ForceCollection, ST_Multi, ST_CollectionHomogenize/Extract (`ST_ForceCollection('POINT(1 2)')` must stay `GEOMETRYCOLLECTION(POINT(1 2))`). The same bug affects `ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))')` in G4. |
| G6 | `util/` module ownership; `data_types.rs` helpers | R1–R4. |
| G5 | — (G5 depends on G1) | ST_DumpPoints/DumpRings/DumpSegments reuse G1 accessors; ST_3DExtent reuses the fixed `BoundingRect`; the `\|=\|` operator reuses ST_DistanceCPA. ST_MakeLine/ST_Collect scalar overloads stay in G5 because DataFusion resolves scalar UDFs before aggregates of the same name. |
| G3 | — | ST_IsSimple/ST_IsRing/ST_WrapX move there. |

## 6. Phasing

Each batch is a series of PRs (one function or one tight family each, per the style guide).

**Batch 0: refactors and fixes (no new functions)**

1. Add `util/{kernel,builder,ordinates,owned}.rs` and `geometry_and` (R1–R4), with unit tests.
2. Delete dead code (R8).
3. Migrate every existing G1 file to the template and fix the bugs in 2.2: X/Y/Z/M, StartPoint/
   EndPoint, GeometryType/ST_GeometryType, CoordDim/NDims, IsClosed, IsEmpty, NPoints (drop the
   alias), NumInteriorRings, the bounds utility and extrema (R7), MakeBox2D/3DMakeBox CRS, and
   the point constructors (nulls, interleaved M, SRID). Apply R5, R6 and R10.
4. Hand-written slt edge cases for every existing function (section 8), recorded from PostGIS.

**Batch 1: simple accessors and bbox (S, used everywhere, unlock doc tests without EWKT)**

ST_Dimension, ST_HasZ, ST_HasM, ST_Zmflag, ST_IsCollection, ST_NumGeometries, ST_GeometryN,
ST_NRings, ST_ExteriorRing, ST_InteriorRingN, ST_NumPoints, ST_PointN, ST_Points, ST_Envelope,
ST_BoundingDiagonal, ST_IsPolygonCW, ST_IsPolygonCCW, ST_Expand, ST_MakeEnvelope.

**Batch 2: coordinate transforms (`map_coords`)**

ST_Affine, ST_Translate, ST_Scale, ST_Rotate, ST_RotateX, ST_RotateY, ST_RotateZ, ST_TransScale,
ST_FlipCoordinates, ST_SwapOrdinates, ST_Force2D, ST_Force3D, ST_Force3DZ, ST_Force3DM,
ST_Force4D, ST_ShiftLongitude, ST_Reverse, ST_ForcePolygonCW, ST_ForcePolygonCCW, ST_ForceRHR,
ST_SnapToGrid, ST_QuantizeCoordinates.

**Batch 3: structural editors and constructors**

ST_Multi, ST_ForceCollection, ST_CollectionExtract, ST_CollectionHomogenize (after the upstream
GC fix), ST_AddPoint, ST_RemovePoint, ST_SetPoint, ST_RemoveRepeatedPoints, ST_Segmentize,
ST_LineExtend, ST_Scroll, ST_Project, ST_RemoveSmallParts, ST_Boundary, ST_ChaikinSmoothing,
ST_MakePolygon (one-argument form first), ST_Polygon, ST_LineFromMultiPoint, ST_TileEnvelope,
ST_Hexagon, ST_Square, ST_Summary.

**Batch 4: linear referencing, trajectories, simple 3D measures**

Segment walker helper; ST_LineInterpolatePoint, ST_LineInterpolatePoints,
ST_3DLineInterpolatePoint, ST_LineSubstring, ST_AddMeasure, ST_InterpolatePoint, ST_LocateAlong,
ST_LocateBetween, ST_LocateBetweenElevations, ST_FilterByM, ST_3DLength, ST_3DPerimeter,
ST_IsValidTrajectory, ST_ClosestPointOfApproach, ST_DistanceCPA, ST_CPAWithin.

**Batch 5: hard native algorithms**

3D distance kernel; ST_3DDistance, ST_3DMaxDistance, ST_3DClosestPoint, ST_3DShortestLine,
ST_3DLongestLine, ST_3DDWithin, ST_3DDFullyWithin, ST_3DIntersects;
ST_RemoveIrrelevantPointsForView. Curve shims if the maintainer wants them (1.4).

## 7. Per-function notes

Algorithm source is the PostGIS reference docs and behaviour observed on PostGIS 3.6.4, unless
noted. **Don't port liblwgeom code**: PostGIS is GPL-2.0. JTS (EDL, BSD-style) can be cited.
"Doc" is the current doc-test parity. Difficulty: S < 1 day, M a few days, L a week or more.

### Accessors

| Function | Algorithm | PostGIS gotchas | Size | Doc |
|---|---|---|---|---|
| GeometryType | type name | Suffix `M` for XYM only; Z and ZM get none. Rect → POLYGON. | S (fix) | 1/3 |
| ST_GeometryType | type name | `ST_MultiLineString` casing. | S (fix) | 1/4 |
| ST_CoordDim / ST_NDims | `dim().size()` | smallint; per-row for Geometry arrays; NULL in, NULL out. | S (fix) | 1/2, 0/1 |
| ST_Dimension | max topological dimension | GC → max of members; `GEOMETRYCOLLECTION EMPTY` → 0. | S | 0/1 |
| ST_HasZ / ST_HasM | from `dim()` | EMPTY still has a dimension (`POINT Z EMPTY` → true). | S | 0/2, 0/2 |
| ST_Zmflag | 0/1/2/3 | smallint; 1 = M, 2 = Z. | S | 0/4 |
| ST_IsCollection | type check | true for MULTI* and GC, even EMPTY. | S | — |
| ST_IsEmpty | recursive emptiness | Already right; migrate only. | S | 4/5 |
| ST_IsClosed | first == last | Compares Z for 3D (not M); points, polygons → true; GC → all members closed; EMPTY line → false. | S (fix) | 0/2 |
| ST_NPoints | count coords | integer; `POINT EMPTY` → 0. | S (fix) | — |
| ST_NumPoints | LineString coords | NULL for anything but LINESTRING. Separate UDF. | S | 1/1 |
| ST_NumGeometries | count parts | Single → 1; topologically EMPTY → 0, otherwise structural (`MULTIPOINT(EMPTY, (1 1))` → 2). | S | 0/2 |
| ST_GeometryN | nth part | 1-based; non-collection with n=1 → itself; out of range → NULL. Output Geometry. | S | 0/2 |
| ST_NRings | rings incl. exterior | Counts all rings of a MULTIPOLYGON; non-polygons and `POLYGON EMPTY` → 0. | S | 0/1 |
| ST_NumInteriorRing(s) | interiors | integer; MULTIPOLYGON → NULL; `POLYGON EMPTY` → 0. | S (fix) | — |
| ST_ExteriorRing | exterior ring | Non-polygon → NULL; `POLYGON EMPTY` → `LINESTRING EMPTY`. | S | — |
| ST_InteriorRingN | nth interior | 1-based; non-polygon → NULL. | S | 0/1 |
| ST_PointN | nth coord | LINESTRING only; 1-based; negative counts from end; 0 / out of range → NULL. | S | 0/3 |
| ST_StartPoint | first coord | Works on any geometry with coordinates (POINT, MULTIPOINT, POLYGON, MULTILINESTRING). | S (fix) | 1/4 |
| ST_EndPoint | last coord | LINESTRING only; EMPTY → NULL. | S (fix) | 1/3 |
| ST_X / ST_Y / ST_Z / ST_M | ordinate | Error for non-point; `POINT EMPTY` → NULL; Z of XYM → NULL; nullable field. | S (fix) | 0/2, 0/2, 0/1, 0/1 |
| ST_Points | all coords | Keeps ring closing points and Z/M; EMPTY → `MULTIPOINT EMPTY`. | S | — |
| ST_Envelope | bbox | POINT → POINT; degenerate vertical/horizontal → LINESTRING; EMPTY → same EMPTY; SRID kept. | S | 0/6 |
| ST_BoundingDiagonal | bbox min → max | `fits` ignored (always exact here); Z/M included if present; EMPTY → `LINESTRING EMPTY`. | S | 0/1 |
| ST_IsPolygonCW / CCW | signed ring area | Non-polygons → true; exterior and interiors have opposite orientation. | S | — |
| ST_Boundary | rings / endpoints | Polygon → LINESTRING or MULTILINESTRING; line → MULTIPOINT of endpoints (mod-2 rule for multilines, closed → `MULTIPOINT EMPTY`); POINT → `POINT EMPTY`; GC → error (record). | M | 0/6 |
| ST_Summary | text | `Point[S]` flags Z, M, B, G, S; nested lines for collections. B (cached bbox) never applies here. | M | — |
| ST_HasArc, ST_CurveN, ST_NumCurves, ST_NumPatches, ST_PatchN | — | Not supported (1.4). | — | 0/1 each |

### Bounding box

| Function | Algorithm | PostGIS gotchas | Size | Doc |
|---|---|---|---|---|
| ST_XMin … ST_ZMax | `BoundingRect` | EMPTY → NULL; Z of 2D → 0; M never counts as Z. | S (fix) | — |
| ST_MakeBox2D / ST_3DMakeBox | two points | Propagate CRS; NULL point → NULL. | S (fix) | —, 1/1 |
| ST_Expand | grow bbox | Overloads: box2d, box3d, geometry; `(geom, dx, dy, dz, dm)`; geometry → polygon (degenerate → point/line); EMPTY → EMPTY; negative may collapse. | M | — |

### Constructors

| Function | Algorithm | PostGIS gotchas | Size | Doc |
|---|---|---|---|---|
| ST_Point / ST_PointZ / ST_PointM / ST_PointZM / ST_MakePoint / ST_MakePointM | coords | NULL in any coordinate → NULL; `srid` scalar; named `srid =>`. | S (fix) | 2/6, 2/3, 2/3, 2/3, 3/4, 1/3 |
| ST_MakeEnvelope | 5-point ring | `srid` optional scalar; ring order (0 0,0 1,1 1,1 0,0 0). | S | 0/1 |
| ST_MakePolygon | shell (+ holes) | Shell must be closed with ≥ 4 points, otherwise error; Z/M kept; holes array needs G6. | M | 0/5 |
| ST_Polygon | MakePolygon + SRID | `srid` scalar. | S | 0/2 |
| ST_LineFromMultiPoint | points → line | Keeps Z/M; non-multipoint → error. | S | 0/1 |
| ST_TileEnvelope | tile math | Default bounds are SRID 3857 (`-20037508.342789244…`); `margin`; out-of-range tile → error. | M | 0/2 |
| ST_Hexagon / ST_Square | grid cell math | `origin` default `POINT(0 0)`; output SRID from origin. | M | 0/1, 0/1 |
| ST_Letters | — | Not supported (1.4). | — | 0/2 |

### Editors

| Function | Algorithm | PostGIS gotchas | Size | Doc |
|---|---|---|---|---|
| ST_FlipCoordinates | swap x, y | Keeps Z/M. | S | 0/1 |
| ST_SwapOrdinates | swap named ordinates | `ords` text ('xy', 'zm', …) scalar; missing ordinate → error. | S | 0/1 |
| ST_Force2D / 3D / 3DZ / 3DM / 4D | set dimension | 3D drops M; 3DM drops Z; `zvalue`/`mvalue` defaults 0; SRID kept. | S | 0/2 each |
| ST_ForceCollection | wrap in GC | Needs the upstream single-element GC fix. | S | 0/3 |
| ST_Multi | single → multi | GC unchanged; same upstream fix. | S | 0/1 |
| ST_CollectionExtract | filter by type | Type 1/2/3 or highest dimension; always MULTI; non-collection of the wrong type → typed EMPTY (`ST_CollectionExtract('POINT(0 0)', 2)` → `LINESTRING EMPTY`). | M | 0/3 |
| ST_CollectionHomogenize | simplest type | GC of one → that geometry; homogeneous → MULTI; nested GC unsupported. | M | 0/5 |
| ST_ForcePolygonCW / CCW / ST_ForceRHR | reverse rings | Non-polygons unchanged; RHR = exterior CW. | S | —, —, 0/1 |
| ST_Reverse | reverse vertex order | Lines and rings; points/multipoints unchanged. | S | 0/1 |
| ST_AddPoint | insert | 0-based `position`, -1 (default) appends; out of range → error. | S | 0/1 |
| ST_RemovePoint | delete | 0-based; result with < 2 points → error. | S | — |
| ST_SetPoint | replace | 0-based; negative counts from end. | S | 0/3 |
| ST_RemoveRepeatedPoints | dedupe | `tolerance` (2D distance); rings keep ≥ 4 points; multipoint duplicates removed; Z/M of the kept point. | M | 0/4 |
| ST_Segmentize | densify | Splits each segment into equal parts; interpolates Z and M. Geography overload → G2 once geography exists. | M | 0/3 |
| ST_SnapToGrid | round to grid | Four overloads incl. point origin with Z/M sizes; `rint` (ties to even); removes repeated points and collapsed parts (lines < 2, rings < 4 points). | M | — |
| ST_QuantizeCoordinates | mantissa masking | Per-ordinate precision; algorithm described in the docs. | M | 0/2 |
| ST_ShiftLongitude | ±360 per coord | x < 0 → x + 360, x > 180 → x − 360. | S | 0/1 |
| ST_LineExtend | extend ends | `distance_forward`, `distance_backward`; direction from end segments. | S | 0/1 |
| ST_Scroll | rotate ring start | Closed line only; point must be a vertex, otherwise error. | S | 0/1 |
| ST_Project | planar azimuth/distance | `(geom, distance, azimuth)` and `(geom1, geom2, distance)`; geography overloads need G6. | S | 0/1 |
| ST_RemoveSmallParts | drop parts by bbox size | Uses bbox width/height, not area; keeps the geometry type. | S | 0/2 |
| ST_RemoveIrrelevantPointsForView | view-dependent simplification | `bounds` box2d; `cartesian_hint`. | L | 0/5 |
| ST_CurveToLine / ST_ForceSFS / ST_LineToCurve / ST_ForceCurve | — | 1.4. | — | 0/4, —, 0/2, 0/1 |

### Affine transformations

All build an `Affine3D` with the same formula as PostGIS's SQL wrappers (ST_Rotate is
`ST_Affine(cos(a), -sin(a), 0, sin(a), cos(a), 0, 0, 0, 1, ...)`), so floating-point results
match bit for bit (`ST_Rotate('POINT(1 0)', pi()/2)` → `POINT(6.123233995736766e-17 1)`). Z is
transformed only if present; M is untouched except by ST_Scale with an M factor.

| Function | PostGIS gotchas | Size | Doc |
|---|---|---|---|
| ST_Affine | 12- and 6-parameter overloads. | S | 0/2 |
| ST_Translate | 2- and 3-offset overloads. | S | 0/4 |
| ST_Scale | Float overloads; `(geom, factor point)` scales M with a ZM factor; `(geom, factor, origin)`. | M | 0/4 |
| ST_Rotate | `(geom, a)`, `(geom, a, x0, y0)`, `(geom, a, origin point)`. | S | 0/3 |
| ST_RotateX / Y / Z | 2D input stays 2D (Z treated as 0). | S | 0/1, 0/1, 0/2 |
| ST_TransScale | 2D: `(x + dx) * xf`. | S | 0/2 |

### Linear referencing, trajectories, processing, 3D measures

| Function | Algorithm | PostGIS gotchas | Size | Doc |
|---|---|---|---|---|
| ST_LineInterpolatePoint | walk segments (2D length) | Fraction outside [0, 1] → error; interpolates Z and M. | S | 0/3 |
| ST_LineInterpolatePoints | repeated | `repeat` default true; returns MULTIPOINT (or POINT for one). | S | — |
| ST_3DLineInterpolatePoint | walk segments (3D length) | | S | 0/1 |
| ST_LineSubstring | cut by fractions | Start > end → error; interpolated endpoints carry Z/M. | M | 0/4 |
| ST_AddMeasure | linear M by length | MULTILINESTRING uses total length; output gains M. | S | 0/4 |
| ST_InterpolatePoint | project point, read M | Returns float; input without M → error. | S | 0/1 |
| ST_LocateAlong | points at M | `offset` perpendicular; returns MULTIPOINT. | M | 0/1 |
| ST_LocateBetween / ST_LocateBetweenElevations | clip by M / Z range | Returns multi or GC; polygons clipped too (record). | M | 0/2, 0/2 |
| ST_FilterByM | drop vertices outside range | `max` NULL = unbounded; `returnM`; no M → error. | S | 0/1 |
| ST_IsValidTrajectory | M strictly increasing | LINESTRINGM/ZM only. | S | 0/2 |
| ST_ClosestPointOfApproach / ST_DistanceCPA / ST_CPAWithin | relative motion over shared M range | Invalid trajectory → error; disjoint time ranges → NULL (CPA) / false. | M | 0/1 each |
| ST_ChaikinSmoothing | corner cutting | `nIterations` (max 5), `preserveEndPoints`; smooths Z and M; points unchanged. | S | 0/3 |
| ST_3DLength / ST_3DPerimeter | sum of 3D segment lengths | 2D input → 2D length. | S | 0/1, 0/1 |
| ST_3DDistance, ST_3DMaxDistance, ST_3DClosestPoint, ST_3DShortestLine, ST_3DLongestLine, ST_3DDWithin, ST_3DDFullyWithin, ST_3DIntersects | 3D distance kernel (segment–segment, point–plane for polygon interiors) | 2D inputs treated as Z = 0; ST_3DIntersects is distance = 0. | L (kernel) + S each | 0/2, 0/1, 0/3, 0/3, 0/3, 0/1, 0/1, 0/2 |

## 8. Testing

### Hand-written slt (`slt/geodatafusion/<function>.slt`)

Besides the style guide's checklist, every G1 file covers:

- **Z vs M:** the same query on `POINT Z`, `POINTM` and `POINT ZM` (and the line/polygon
  equivalents). `nth(2)` bugs only show up when XYM is tested next to XYZ.
- **Mixed dimensions in one column:** `SELECT ST_Foo(g) FROM (VALUES ('POINT(1 2)'::geometry),
  ('POINT Z(1 2 3)'::geometry), ('LINESTRINGM(0 0 1, 1 1 2)'::geometry), (NULL)) t(g)`, which
  produces a Geometry array with several children and a NULL row.
- **EMPTY of every type**, including `MULTIPOINT(EMPTY, 1 1)`-style partially empty multis (via
  WKB hex literals while WKT parsing can't express them).
- **NULL in every argument position**, not just the geometry.
- **Index arguments:** 0, 1, last, last + 1, negative, for every 0- or 1-based function.
- **Single-element collections:** `GEOMETRYCOLLECTION(POINT(1 2))`, `MULTIPOINT(1 2)`. These
  catch the GeometryBuilder collapse.
- **SRID propagation** with `'SRID=4326;...'` literals, once G4's EWKT parsing lands.
- **PostGIS errors** (`ST_X` on a line, `ST_MakePolygon` on an open line) recorded as
  `query error`.
- **Floating-point identity** for affine functions (`pi()/2` rotations), which checks that the
  matrix is built the PostGIS way.

### Unit tests (Rust API)

Only what SQL can't show:

- Return field: Arrow type, extension type, nullability (the X/Y non-nullable bug), CRS from the
  input field.
- `same_type_output`: LineString in → LineString out with the same dimension; WKB in → Geometry
  out.
- `coord_type`: one test per geometry-returning UDF with `CoordType::Interleaved`. The
  ST_MakePointM panic lived on the untested interleaved path.
- Helpers in `util/` get their own tests: `z`/`m` on every dimension, `map_coords` structure and
  dimensions for each type including empties.

A small `#[cfg(test)]` helper (`run_udf(udfs, sql) -> RecordBatch`) would remove the
`SessionContext`/`collect`/`unwrap` boilerplate repeated in every test module.

## 9. Style guide amendments

1. **Layout:** add "Helpers shared across providers live in `src/util/` (`kernel`, `builder`,
   `ordinates`, `owned`)" and list the new categories (`editors`, `affine_transformations`,
   `linear_referencing`, `trajectory`). *Rationale:* the current rule (category `util/` or
   `data_types.rs`) has no place for crate-wide geometry helpers.
2. **Per-geometry loops:** "Implement `GeometryKernel` and call `map_geometry` (Arrow output) or
   `map_geometry_to_geoarrow` (GeoArrow output). Write a loop by hand only when the kernel
   doesn't fit (aggregates, multi-row outputs), and name the per-array function
   `impl_<file_name>`." *Rationale:* one place for NULL and error handling (R1).
3. **Return types:** "Map PostGIS SQL types: `integer` → Int32, `smallint` → Int16, `bigint` →
   Int64, `float8` → Float64, `boolean` → Boolean, `text` → Utf8. Never return unsigned
   integers." *Rationale:* R5.
4. **Nullability:** "Return fields are nullable unless the function can never return NULL."
   *Rationale:* X/Y/Z/M declare non-nullable fields and return NULLs.
5. **Z and M:** "Never read Z or M with `CoordTrait::nth`; use `util::ordinates::{z, m}`.
   Build owned geometries as `wkt::Wkt<f64>` via `util::owned`." *Rationale:* four M-as-Z bugs.
6. **Geometry output type:** extend "Return the most specific GeoArrow type" with "A
   type-preserving function returns `same_type_output(input, coord_type)`: the input's native
   type, or `GeometryType` for WKB/WKT input." *Rationale:* makes R2 the rule.
7. **Arguments:** "Destructure arguments with `take_function_args` when the arity is fixed. Read
   per-row values from the broadcast arrays; only arguments that change the return type (SRID)
   may be scalar-only." Add `geometry_and(...)` to the Signatures bullet, and "use
   `with_parameter_names` exactly where PostGIS names the parameters". *Rationale:* R4, R6, R9.
8. **Errors:** "Where PostGIS raises an error, raise `Execution` with a message naming the SQL
   function; don't return NULL instead." *Rationale:* ST_X/ST_PointN return NULL where PostGIS
   errors.
9. **Floating point:** "When PostGIS defines a function in terms of another (ST_Rotate via
   ST_Affine), compute it the same way so results match exactly." *Rationale:* rendering keeps
   12 significant digits, so `6.123e-17` vs `0` is a diff.
10. **Licensing:** "Implement from the PostGIS documentation and observed behaviour. Don't copy
    or translate liblwgeom/PostGIS source (GPL-2.0). JTS (EDL) and GEOS (LGPL) may be
    referenced in comments." *Rationale:* the crate is MIT/Apache-2.0.
11. **Struct doc comments:** state that the existing one-line `///` rule applies to every
    existing struct too (only `Dump` has one).

## 10. Open questions for the maintainer

1. Do you accept the reassignments in 1.2 and 1.3, especially the M/Z linear-referencing
   functions and the 3D measurement family moving from G2 to G1? Should ST_LineLocatePoint move
   too, for family consistency?
2. Are the breaking return-type changes (UInt8/UInt32 → Int16/Int32, Utf8View → Utf8) fine for
   the next release?
3. Should the shared helpers live in a crate-wide `src/util/` (proposed), or under
   `udf/native/util/` until G2 needs them? G6 owns scaffolding; please confirm G1 may add it.
4. The GeometryBuilder single-element collection collapse and the `MULTIPOLYGON(EMPTY, ...)`
   panic are geoarrow-rs bugs. Should we fix them upstream (and wait for a release) or work
   around them locally, for example with our own GeometryCollection output path?
5. Should we add shims for curve functions (ST_HasArc → false, ST_CurveToLine/ST_ForceSFS →
   identity)? They're harmless but unlock no tests.
6. ST_Letters and ST_MemSize: declare permanently unsupported?
7. Should ST_X/ST_Y/ST_Z/ST_M (and similar accessors) error on wrong geometry types like
   PostGIS, even though that turns some of today's NULL results into query errors?
8. For type-preserving functions, is the per-type output (`same_type_output`) worth it, or
   should every geometry-returning G1 function return `GeometryType` for simplicity?
9. ST_Expand and other box functions depend on how G6 models `box2d`/`box3d`. Is `RectArray`
   with `BoxType` the long-term representation?
