# G3: External C libraries (GEOS, PROJ)

FFI wrappers around [GEOS](https://libgeos.org/) (through the `geos` crate) and
[PROJ](https://proj.org/) (through the `proj` crate), behind Cargo features. The only existing
function is `ST_LineMerge` (`udf/geos/processing/line_merge.rs`).

Facts this plan relies on, checked while writing it:

| | Version |
|---|---|
| `geos` crate in `Cargo.lock` | 11.1.1 (latest 11.3.1), `geos-sys` 2.0.9 |
| GEOS on this machine | 3.15.0 |
| GEOS on CI (`ubuntu-latest` = 24.04, `libgeos-dev`) | 3.12.1 |
| GEOS in the PostGIS oracle (`postgis/postgis:18-3.6`) | 3.14.1 at runtime, PostGIS compiled against 3.13.1 |
| GEOS bundled by `geos/static` | `geos-src` 0.2.4 = 3.14.1, `geos-src` 0.2.5 = 3.15.1dev |
| PROJ on this machine / in the oracle / on CI | 9.8.1 / 9.8.1 / 9.4.0 |
| `proj` crate | 0.31.0, needs PROJ >= 9.6.2 (otherwise `proj-sys` builds PROJ 9.6.2 from source) |

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### Functions in G3

All of them are GEOS-backed unless marked PROJ.

| Chapter | Functions |
|---|---|
| overlay | ST_Intersection, ST_Difference, ST_SymDifference, ST_Union (two-geometry scalar forms only), ST_UnaryUnion, ST_Node, ST_Split, ST_ClipByBox2D |
| processing | ST_LineMerge, ST_Buffer, ST_OffsetCurve, ST_BuildArea, ST_DelaunayTriangles, ST_VoronoiPolygons, ST_VoronoiLines, ST_TriangulatePolygon, ST_ReducePrecision, ST_SharedPaths, ST_SimplifyPolygonHull, ST_MaximumInscribedCircle, ST_LargestEmptyCircle, **ST_ConcaveHull**, **ST_SimplifyPreserveTopology** |
| editor | ST_Normalize, ST_Snap |
| measurement | ST_MinimumClearance, ST_MinimumClearanceLine |
| validation | ST_MakeValid, ST_IsValidDetail, **ST_IsValid**, **ST_IsValidReason** |
| accessor | **ST_IsSimple**, **ST_IsRing** |
| srs (PROJ) | ST_Transform, ST_TransformPipeline, ST_InverseTransformPipeline, **postgis_srs, postgis_srs_all, postgis_srs_codes, postgis_srs_search** |

Bold functions are proposed reassignments into G3.

### Reassignments

| Function | From → to | Why |
|---|---|---|
| ST_ConcaveHull | G2 → G3 | PostGIS 3.6 calls `GEOSConcaveHull` (GEOS 3.11). `geo`'s concave hull is a different algorithm with a different parameter (`param_pctconvex` vs concavity), so it can't reach parity (0/3). |
| ST_SimplifyPreserveTopology | G2 → G3 | PostGIS uses GEOS's Douglas-Peucker-based `TopologyPreservingSimplifier`. The current code calls `geoarrow_expr_geo::simplify_vw_preserve`, a Visvalingam-Whyatt algorithm. That's a different function, not a rounding difference (0/3). |
| ST_IsValid, ST_IsValidReason | G2 → G3 (proposed, agree with G2) | PostGIS uses `GEOSisValidDetail`. The reason strings (`Self-intersection[0.5 0.5]`) and the `flags` argument (ESRI validity) are GEOS's. One mechanism for the whole validation chapter, together with ST_IsValidDetail. |
| ST_IsSimple, ST_IsRing | G1 → G3 (proposed, agree with G1) | PostGIS uses `GEOSisSimple`/`GEOSisRing`. Simplicity needs noding, which is a big native job. |
| postgis_srs* | G1 → G3 | They read the PROJ database. Blocked (see below). |
| ST_Union (aggregate), ST_MemUnion, ST_Polygonize, ST_Coverage*, ST_Subdivide, ST_ClusterIntersecting(Win), ST_ClusterWithin(Win) | stay G5 | The UDF kind decides the code shape. Their per-row kernels use the G3 bridge (section 4). |
| ST_MaximumInscribedCircle, ST_LargestEmptyCircle, ST_IsValidDetail | stay G3, shared with G5/G6 | They return records. The doc tests call MIC/LEC in `FROM`, which needs a DataFusion table function (G5). The struct type convention is G6's. G3 provides the kernel and a struct-returning scalar UDF. |

Candidates that PostGIS also runs through GEOS but that can stay in G1/G2 if those groups match
GEOS's output (vertex order, start point): ST_ConvexHull, ST_PointOnSurface, ST_OrientedEnvelope,
ST_Boundary, ST_Equals, ST_Relate (the `boundaryNodeRule` overload needs GEOS),
ST_HausdorffDistance and ST_FrechetDistance (the `densifyFrac` overloads need GEOS). See
[geo vs GEOS](#geo-vs-geos).

### Can't be supported yet

| Function / overload | Blocker |
|---|---|
| ST_TriangulatePolygon, ST_SimplifyPolygonHull, ST_LargestEmptyCircle, ST_IsValidDetail, ST_IsValid(geom, flags) | The `geos` crate doesn't wrap `GEOSConstrainedDelaunayTriangulation_r`, `GEOSPolygonHullSimplify_r`, `GEOSLargestEmptyCircle_r`, `GEOSisValidDetail_r` (not even in 11.3.1). Its `AsRaw` trait is `pub(crate)`, so we can't call `geos::sys` on a `geos::Geometry`. Needs an upstream PR (preferred) or our own FFI. |
| ST_MaximumInscribedCircle, ST_LargestEmptyCircle, ST_IsValidDetail | Record return types (G6) and `FROM` usage (G5 table function). The harness renderer has no struct rendering. |
| ST_DelaunayTriangles(…, flags => 2) | Returns a TIN. GeoArrow has no TIN/Triangle type. `NotImplemented`. |
| ST_InverseTransformPipeline | The `proj` crate only transforms forward (`convert`). |
| ST_Transform of geometries with Z | The `proj` crate is 2D only (`convert` passes `z: 0.0`). PostGIS transforms Z (`SRID=4326;POINT Z` → 4978 gives a geocentric Z). |
| postgis_srs* | Need `proj_get_crs_info_list_from_database`, which the `proj` crate doesn't expose. |
| Geography overloads (ST_Buffer(geography), ST_Intersection(geography), …) | Need the geography type (G6) and `_ST_BestSRID` + PROJ. |
| ST_CoverageClean (G5) | The oracle's PostGIS was compiled against GEOS 3.13, so it raises an error. No expected output can be recorded. |
| Curved geometries in every function | Not in GeoArrow. |

## 2. Existing basis

`rust/geodatafusion/src/udf/geos/processing/line_merge.rs`:

- `:21-30` builds a `Signature::one_of` with an `Exact` variant for each of the 74 geometry types,
  with and without a `Boolean` (148 variants). That works for one geometry argument, but a
  two-geometry function would need 74 × 74 variants. `ST_Intersects` gets around this with
  `Signature::any(2)`, which the style guide forbids.
- `:52-85` is the standard UDF anatomy (`coord_type`, `Default`, thin trait methods). It's correct.
- `:77` says "This function strips the M dimension". It strips Z too (see `:151`).
- `:87-94` `return_field_impl` is the same as in `centroid.rs`, `convex_hull.rs` and others.
- `:99-112` `parse_directed` treats NULL `directed` as `false`. PostGIS functions are `STRICT`, so
  `ST_LineMerge(geom, NULL)` is NULL (checked on the oracle). Array-valued `directed` returns
  `NotImplemented`, and `:105` uses `unreachable!`.
- `:118-128` broadcasts scalars to arrays and converts the whole column with `to_wkb::<i32>`. That
  materialises native arrays as WKB, narrows `LargeWkb` to i32 offsets (fails above 2 GiB), and
  converts a scalar geometry once per row.
- `:136` `Geometry::new_from_wkb` reads M on GEOS >= 3.12 and ignores it before that.
- `:137-143` returns empty input unchanged. This matches PostGIS, including M
  (`ST_LineMerge('LINESTRING M EMPTY')` is `LINESTRING M EMPTY`).
- `:151` `Geom::to_wkb()` calls `GEOSGeomToWKB_buf_r`, which writes the context's default output
  dimension of 2. **Z is dropped.** Verified with a probe on GEOS 3.15:
  `POINT Z (1 2 3)` → `0101000000…` (2D). PostGIS keeps Z:
  `ST_LineMerge('MULTILINESTRING Z ((0 0 1,1 1 2),(1 1 2,2 2 3))')` is
  `LINESTRING Z (0 0 1,1 1 2,2 2 3)`. The fifth doc test (`postgis_docs/st_linemerge.slt:40`) now
  fails on WKT parsing (G4). Once that's fixed it will fail here.
- `:157-159` collects `Vec<Option<Vec<u8>>>` into a `BinaryArray`, wraps it as `WkbArray` and
  casts with `from_wkb`, which is two more passes.
- `:163-300` unit tests restate PostGIS behaviour (doc examples, NULL, directed). Under the style
  guide, behaviour belongs in `slt/geodatafusion/st_linemerge.slt`, which doesn't exist.
- There's no Python binding (the Python crate enables no GEOS feature). README marks it ✅.

Wiring:

- `udf/mod.rs:3-4` gates `pub mod geos` on `geos-3_11`. `udf/geos/processing/mod.rs:3,7` gates
  the function again. `lib.rs:23-24` registers `geos::processing`.
- `error.rs:22,39` gate the `Geos` variant on `feature = "geos"`. **That feature does exist.** It's
  the implicit feature Cargo creates for the optional `geos` dependency, because nothing uses
  `dep:geos` (`cargo metadata` lists `geos = ["dep:geos"]`). `geos-3_11 = ["geos/v3_11_0"]` turns
  it on, so the code compiles. But `--features geos` alone is accepted and enables no UDFs, and
  STYLE_GUIDE.md wrongly says the feature doesn't exist.
- `ci.yml` installs `libgeos-dev` (3.12.1) and runs everything with `--all-features`.
  `geos-sys` panics at build time if the system GEOS is older than the newest requested
  `v3_x_0` feature, so adding any `geos-3_13`+ feature would break CI.

## 3. Refactoring assessment

### R1. A shared GEOS bridge (recommended, M effort, low risk)

Replace the WKB round trip with three helpers in `udf/geos/util/` (section 4):

- **`to_geos`** converts a geo-traits geometry straight to GEOS (`CoordSeq::new_from_buffer`,
  `Geometry::create_*`). It keeps Z and drops M, which is what PostGIS's `LWGEOM2GEOS` does. Every
  GEOS-backed PostGIS function drops M and keeps Z (checked on the oracle for ST_Intersection,
  ST_Union, ST_Node, ST_Snap, ST_ReducePrecision, ST_Normalize, ST_MakeValid, ST_OffsetCurve,
  ST_ClipByBox2D, ST_LineMerge, ST_UnaryUnion, ST_DelaunayTriangles). WKB can't do this. GEOS >=
  3.12 reads M from WKB and keeps it through operations: a probe shows GEOS 3.15
  `line_merge` returning `LINESTRING M (…)`, where PostGIS returns 2D.
- **`GeosColumn`** converts one geometry argument. A scalar is converted once, not once per row.
  `get(i)` returns `Option<&geos::Geometry>`, and the source array is kept so the input can be
  returned unchanged.
- **`GeosGeometryBuilder`** writes GEOS results with a `WKBWriter` set to
  `CoordDimensions::ThreeD` (Z kept, and M can't occur), reads them with
  `wkb::reader::read_wkb` and pushes them into a `GeometryBuilder`. That's one pass, with no
  intermediate `BinaryArray`.

The bridge was compiled and run in a scratch crate against `geos` 11.1.1, `geoarrow-array` 0.8
and `wkb` 0.9. `LINESTRING ZM` came out as `LINESTRING Z`, `POLYGON M` as `POLYGON`, and NULLs
and EMPTYs were preserved. `CoordSeq::new_from_buffer` needs `geos/v3_10_0`, which `geos-3_11`
already implies.

Every function then becomes the same explicit row loop (section 4). NULL, EMPTY and parameter
handling stay visible in each file, as the style guide asks, and the bridge code lives in one
place.

Before (line_merge.rs):

```rust
let arrays = ColumnarValue::values_to_arrays(&args.args[0..1])?;
let geo_array = from_arrow_array(&arrays[0], &args.arg_fields[0])?;
let wkb_array = to_wkb::<i32>(geo_array.as_ref())?;
let mut merged: Vec<Option<Vec<u8>>> = Vec::with_capacity(wkb_array.inner().len());
for maybe_wkb in wkb_array.inner() { /* new_from_wkb, line_merge, to_wkb */ }
let result_wkb = WkbArray::new(merged.into_iter().collect::<BinaryArray>(), metadata);
let result = from_wkb(&result_wkb, to_type)?;
```

After:

```rust
let geom = GeosColumn::try_new(&args.args[0], &args.arg_fields[0])?;
let directed = boolean_arg(args.args.get(1), args.number_rows)?;
let mut builder = GeosGeometryBuilder::try_new(&args.return_field, args.number_rows)?;
for i in 0..args.number_rows {
    // ST_LineMerge is STRICT: SQL NULL in any argument gives SQL NULL.
    let (Some(g), Some(directed)) = (geom.get(i), directed.get(i)) else {
        builder.push_null();
        continue;
    };
    // PostGIS returns empty input unchanged (including M), whereas GEOS would collapse it to
    // an empty GeometryCollection.
    if g.is_empty()? {
        builder.push_input(&geom, i)?;
        continue;
    }
    let merged = if directed { g.line_merge_directed()? } else { g.line_merge()? };
    builder.push_geos(&merged)?;
}
Ok(ColumnarValue::Array(builder.finish()))
```

This fixes the Z bug, the NULL `directed` bug and the array `directed` `NotImplemented`, and
removes the `unreachable!`. Risks: GEOS results with mixed dimensions inside one collection make
`geoarrow-array`'s `PointBuilder` panic (`builder/point.rs:99`, an `unwrap` on
`IncorrectGeometryType`). Keeping one `has_z` for the whole input geometry in `to_geos` avoids
creating such inputs, but a binary op on one Z and one 2D geometry could still produce one. Test
it, and report the `unwrap` upstream.

### R2. Feature layout (recommended, S effort, low risk)

```toml
[features]
# Enables GEOS-backed UDFs. Each `geos-3_x` feature enables the UDFs that need at most GEOS 3.x
# and implies the lower versions. Add a version only with the first UDF that needs it.
geos-3_11 = ["dep:geos", "dep:wkb", "geos/v3_11_0"]
# Builds GEOS from source and links it statically, instead of linking the system library.
geos-static = ["geos-3_11", "geos/static"]
# Enables PROJ-backed UDFs (ST_Transform).
proj = ["dep:proj"]
```

- Keep `geos-3_11` as the floor. Every G3 function (section 7) works with GEOS <= 3.11, it's the
  existing feature, and Debian 12 (3.11.1) and Ubuntu 24.04 (3.12.1) have it. A `geos-3_10` floor
  would add Ubuntu 22.04 (3.10.2) at the cost of a second cfg level for `ST_LineMerge(directed)`
  and `ST_ConcaveHull` (open question).
- `dep:geos` removes the implicit `geos` feature. Gate the whole `udf::geos` module, the bridge,
  and the `Geos`/`Wkb` error variants on `geos-3_11`. Only functions needing a higher version get
  their own `#[cfg]` on `mod`, `pub use` and `register`, which is what the coverage functions in G5
  will need (`geos-3_12`).
- `wkb` becomes an optional normal dependency (it's already a workspace dependency and in the tree
  through `geoarrow-array`).
- `lib.rs` registers each GEOS category under `#[cfg(feature = "geos-3_11")]`, the same way it
  does today.
- `docs.rs`: `all-features = true` would build GEOS (and PROJ) from source on docs.rs. Use
  `features = ["geos-3_11", "proj", "geos/dox"]` instead (`geos/dox` skips linking). The PROJ
  equivalent (`proj-sys/nobuild`) needs a direct `proj-sys` dependency (open question).

### R3. error.rs (recommended, S effort)

```rust
#[cfg(feature = "geos-3_11")]
#[error(transparent)]
Geos(#[from] geos::Error),

#[cfg(feature = "geos-3_11")]
#[error(transparent)]
Wkb(#[from] wkb::error::WkbError),

#[cfg(feature = "proj")]
#[error(transparent)]
ProjCreate(#[from] proj::ProjCreateError),

#[cfg(feature = "proj")]
#[error(transparent)]
Proj(#[from] proj::ProjError),
```

All of them map to `DataFusionError::External`, like `GeoArrow`. GEOS messages
(`IllegalArgumentException: Geometry is not lineal`) don't name the SQL function. PostGIS
prefixes the C function (`lwgeom_sharedpaths: GEOS Error: …`). Wrapping with
`DataFusionError::context("ST_SharedPaths")` would break the `Ok(..?)` thin-method rule, so leave
it unless the maintainer wants it (open question).

### R4. Signatures for multi-argument geometry functions (needed, S effort, coordinate with G6)

`Exact` variants explode for two geometry arguments, and `Signature::any` is banned. DataFusion 54
has `Signature::user_defined` plus `ScalarUDFImpl::coerce_types`, which is the idiomatic route
(`datafusion-expr` 54.1 `udf.rs:987`). Proposed shared helper in `data_types.rs` (G6 owns it; G3
adds it if G6 hasn't yet):

```rust
pub(crate) enum ArgType { Geometry, Float64, Int64, Boolean, Utf8 }

/// Validates geometry arguments and coerces the others for a `Signature::user_defined` UDF.
/// `required` arguments come first, followed by up to `optional.len()` optional arguments.
pub(crate) fn coerce_args(
    name: &str,
    arg_types: &[DataType],
    required: &[ArgType],
    optional: &[ArgType],
) -> Result<Vec<DataType>>
```

Geometry arguments keep their `DataType` (membership in `any_geometry_type()`), so field metadata
survives. Numeric literals (`ST_Buffer(geom, 1)`) are cast to `Float64` by DataFusion.
`Signature::with_parameter_names` could also enable PostGIS named notation
(`ST_Buffer(geom => …, radius => …)`) later.

### R5. Return field and CRS checks (recommended, S effort, coordinate with G6)

One helper replaces the copies of `return_field_impl`:

```rust
/// A `GeometryType` return field with the CRS of the geometry arguments `geometry_args`.
///
/// Errors if they have different CRSs, as PostGIS does for mixed SRIDs.
pub(crate) fn geometry_return_field(
    name: &str,
    args: &ReturnFieldArgs,
    geometry_args: &[usize],
    coord_type: CoordType,
) -> GeoDataFusionResult<FieldRef>
```

PostGIS checks SRIDs per row (`Operation on mixed SRID geometries (4326 != 3857)`). GeoArrow keeps
the CRS per column, so we check at planning time and return `DataFusionError::Plan`. G6 decides
when two CRS values are equal (`EPSG:4326` vs PROJJSON for 4326).

### R6. CI (recommended, S effort, medium risk)

The parity suite compares GEOS output exactly (vertex order, constructed coordinates to 12
significant digits). With GEOS 3.15 locally, 3.12.1 on CI and 3.14.1 in the oracle, `parity.txt`
would differ between machines as soon as ST_Buffer, ST_VoronoiPolygons or ST_MakeValid land.
Proposal:

- `--all-features` includes `geos-static`, so `cargo slt`, `cargo test --all-features`, clippy and
  docs all use the bundled GEOS everywhere.
- Pin the bundled GEOS to the oracle's 3.14.1: `geos-src = 0.2.4` and `geos-sys = 2.0.9` in
  `Cargo.lock` (`geos-sys` 2.0.10 requires `geos-src` >= 0.2.5, which is 3.15.1dev), and add a
  Dependabot `ignore` for both. This holds the `geos` crate at 11.1.x until the oracle image moves
  to GEOS 3.15, at which point both move together and the expectations are re-recorded. Record the
  GEOS version in `tests/sqllogictests/README.md`.
- Static build cost: 1m42s on this 16-thread machine (cmake + C++, preinstalled on
  `ubuntu-latest`), cached afterwards by `Swatinem/rust-cache`.
- Add a job that links the system library and checks the gating:

  ```yaml
  check-features:
    steps:
      - run: sudo apt-get install -y libgeos-dev
      - run: cargo check -p geodatafusion
      - run: cargo check -p geodatafusion --features geos-3_11
  ```

  This catches a function placed under the wrong version `cfg` (it would only fail to compile
  without the higher feature) and keeps dynamic linking tested.
- PROJ (phase 6): Ubuntu's PROJ 9.4 is older than `proj-sys` 0.27's minimum of 9.6.2, so
  `--all-features` would build PROJ from source (cmake, plus the `sqlite3` binary and
  `libsqlite3-dev`, several minutes). Alternatives: a container with a newer PROJ, or keeping
  `proj` out of the default `cargo slt` (open question).

### R7. Python bindings (recommended once the wheel question is settled)

Add `python/src/udf/geos/<category>.rs` with `impl_udf_coord_type_arg!(LineMerge, PyLineMerge,
"LineMerge")`, a `geos` submodule registered like `geo`, and stubs in
`python/python/geodatafusion/geos/`. That needs a Python crate feature
`geos = ["geodatafusion/geos-3_11"]` and a way to ship GEOS in wheels. GEOS is LGPL-2.1, so
linking it statically into an MIT/Apache wheel is legally awkward. Shapely builds GEOS as a shared
library and bundles it with auditwheel/delocate. Until that's decided, GEOS functions stay
Rust-only, and the style guide's "Python binding in the same change" rule needs an exception for G3
(open question).

### R8. Unit tests (recommended, S effort)

Move `ST_LineMerge`'s behavioural cases to `slt/geodatafusion/st_linemerge.slt` (recorded from
PostGIS, adding Z, M, ZM, NULL `directed`, EMPTY variants, SRID). Keep one unit test for the
return field (`GeometryType`, `coord_type`, CRS) and one for scalar/array argument mixing.

### Not recommended

- **A geo fallback when GEOS is disabled.** Two implementations of one SQL function would behave
  differently depending on features, and the parity suite only exercises one of them.
- **Keeping the WKB bridge with an M-stripping writer.** `wkb::writer` 0.9 has no dimension option,
  so it would need a geo-traits adapter, which is more code than `to_geos` and still two passes.
- **Bumping `geos` to 11.3.1 now.** It adds `v3_15_0`, `split` and prepared `relate`, none of which
  G3 needs (PostGIS's ST_Split doesn't use `GEOSSplit`), and it conflicts with pinning GEOS to
  3.14.1.

## 4. Canonical templates

### Module layout

```
rust/geodatafusion/src/udf/geos/
├── mod.rs              //! UDFs implemented via GEOS bindings. Category `pub mod`s and `mod util`.
├── util/
│   ├── mod.rs          private `mod`s and `pub(crate) use`s
│   ├── convert.rs      to_geos
│   ├── column.rs       GeosColumn
│   ├── builder.rs      GeosGeometryBuilder
│   └── params.rs       parse_params, BufferStyle, MakeValidOptions
├── overlay/            intersection.rs, difference.rs, sym_difference.rs, union.rs,
│                       unary_union.rs, node.rs, split.rs, clip_by_box_2d.rs
├── processing/         line_merge.rs, buffer.rs, offset_curve.rs, build_area.rs,
│                       delaunay_triangles.rs, voronoi.rs (Polygons + Lines), reduce_precision.rs,
│                       shared_paths.rs, concave_hull.rs, simplify_preserve_topology.rs,
│                       maximum_inscribed_circle.rs, …
├── editors/            normalize.rs, snap.rs
├── measurement/        minimum_clearance.rs (MinimumClearance + MinimumClearanceLine)
├── validation/         make_valid.rs, is_valid.rs, is_valid_detail.rs
└── accessors/          is_simple.rs, is_ring.rs
rust/geodatafusion/src/udf/proj/
├── mod.rs
├── util.rs             proj_definition, thread-local Proj cache
└── spatial_reference/  transform.rs, transform_pipeline.rs
```

`util` sits at provider level rather than category level because every GEOS category uses it.
`pub(crate)` lets G5's aggregates reuse it.

### Shared helpers

```rust
// udf/geos/util/convert.rs

/// Converts a geometry to GEOS, keeping Z and dropping M, as PostGIS's `LWGEOM2GEOS` does.
///
/// The Z flag of the outer geometry applies to every part, so GEOS never sees mixed dimensions.
pub(crate) fn to_geos(geom: &impl GeometryTrait<T = f64>) -> GeoDataFusionResult<geos::Geometry>;

// udf/geos/util/column.rs

/// A geometry argument converted to GEOS. A scalar is converted once and reused for every row.
pub(crate) struct GeosColumn {
    array: Arc<dyn GeoArrowArray>,
    geometries: Vec<Option<geos::Geometry>>,
    is_scalar: bool,
}

impl GeosColumn {
    pub(crate) fn try_new(value: &ColumnarValue, field: &Field) -> GeoDataFusionResult<Self>;
    /// The geometry in row `i`, or `None` for SQL NULL.
    pub(crate) fn get(&self, i: usize) -> Option<&geos::Geometry>;
    /// The geometry of a scalar argument, for example to prepare it once.
    pub(crate) fn as_scalar(&self) -> Option<Option<&geos::Geometry>>;
}

// udf/geos/util/builder.rs

/// Builds a GeoArrow `GeometryArray` from GEOS geometries.
pub(crate) struct GeosGeometryBuilder {
    builder: GeometryBuilder,
    writer: geos::WKBWriter,
}

impl GeosGeometryBuilder {
    /// Takes the type (CRS, coord type) from the UDF's return field.
    pub(crate) fn try_new(return_field: &Field, capacity: usize) -> GeoDataFusionResult<Self>;
    pub(crate) fn push_geos(&mut self, geom: &impl Geom) -> GeoDataFusionResult<()>;
    /// Copies row `i` of `column`'s source array unchanged, keeping M.
    pub(crate) fn push_input(&mut self, column: &GeosColumn, i: usize) -> GeoDataFusionResult<()>;
    pub(crate) fn push_null(&mut self);
    pub(crate) fn finish(self) -> ArrayRef;
}

// udf/geos/util/params.rs

/// Splits a PostGIS parameter string (`'quad_segs=8 endcap=flat'`) into key/value pairs.
pub(crate) fn parse_params<'a>(params: &'a str) -> impl Iterator<Item = (&'a str, &'a str)>;

/// The `ST_Buffer`/`ST_OffsetCurve` style parameters, with PostGIS defaults and errors.
pub(crate) struct BufferStyle { /* quad_segs, endcap, join, mitre_limit, side */ }

impl BufferStyle {
    pub(crate) fn parse(name: &str, params: &str) -> GeoDataFusionResult<Self>;
    pub(crate) fn to_buffer_params(&self) -> GeoDataFusionResult<geos::BufferParams>;
}
```

Crate-wide helpers that G3 needs first. They live in `data_types.rs` and G6 owns them:
`coerce_args` (R4), `geometry_return_field` (R5), and per-row argument readers:

```rust
/// A `Float64` argument as one value per row. `None` for a missing optional argument.
pub(crate) fn float64_arg(value: Option<&ColumnarValue>, number_rows: usize)
    -> GeoDataFusionResult<Option<Float64Array>>;
// Same for boolean_arg, int64_arg, utf8_arg.
```

These read array-valued arguments too, so functions no longer return `NotImplemented` for
column-valued parameters.

### Template A: unary, geometry in, geometry out (ST_Node)

```rust
use std::sync::OnceLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::scalar_doc_sections::DOC_SECTION_OTHER;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use geoarrow_schema::CoordType;
use geos::{DimensionType, Geom};

use crate::data_types::{any_single_geometry_type_input, geometry_return_field};
use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, GeosGeometryBuilder};

/// Nodes a collection of lines.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Node {
    coord_type: CoordType,
}

impl Node {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl ScalarUDFImpl for Node {
    fn name(&self) -> &str {
        "st_node"
    }

    fn signature(&self) -> &Signature {
        any_single_geometry_type_input()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("return_type".to_string()))
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_return_field("ST_Node", &args, &[0], self.coord_type)?)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(node_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns a (Multi)LineString representing the fully noded version of a collection of linestrings. This function drops the M coordinate.",
                "ST_Node(geom)",
            )
            .with_argument("geom", "geometry")
            .build()
        }))
    }
}

fn node_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geom = GeosColumn::try_new(&args.args[0], &args.arg_fields[0])?;
    let mut builder = GeosGeometryBuilder::try_new(&args.return_field, args.number_rows)?;
    for i in 0..args.number_rows {
        // SQL NULL in, SQL NULL out.
        let Some(geom) = geom.get(i) else {
            builder.push_null();
            continue;
        };
        // PostGIS checks the dimension before calling GEOS.
        if geom.get_dimension()? != DimensionType::Line {
            return Err(DataFusionError::Execution(
                "ST_Node: Noding geometries of dimension != 1 is unsupported".to_string(),
            )
            .into());
        }
        builder.push_geos(&geom.node()?)?;
    }
    Ok(ColumnarValue::Array(builder.finish()))
}
```

### Template B: binary with an optional parameter (ST_Intersection)

Only the parts that differ from A:

```rust
static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

impl ScalarUDFImpl for Intersection {
    // name, signature as above

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(
            "ST_Intersection",
            arg_types,
            &[ArgType::Geometry, ArgType::Geometry],
            &[ArgType::Float64],
        )
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_return_field("ST_Intersection", &args, &[0, 1], self.coord_type)?)
    }
    // documentation: "ST_Intersection(geomA, geomB, gridSize)", arguments geomA, geomB, gridSize
}

fn intersection_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geom_a = GeosColumn::try_new(&args.args[0], &args.arg_fields[0])?;
    let geom_b = GeosColumn::try_new(&args.args[1], &args.arg_fields[1])?;
    let grid_size = float64_arg(args.args.get(2), args.number_rows)?;
    let mut builder = GeosGeometryBuilder::try_new(&args.return_field, args.number_rows)?;
    for i in 0..args.number_rows {
        // ST_Intersection is STRICT: SQL NULL in any argument gives SQL NULL.
        let (Some(a), Some(b)) = (geom_a.get(i), geom_b.get(i)) else {
            builder.push_null();
            continue;
        };
        // Missing gridSize means the PostGIS default of -1 (no snapping).
        let grid_size = match &grid_size {
            None => -1.0,
            Some(grid_size) if grid_size.is_null(i) => {
                builder.push_null();
                continue;
            }
            Some(grid_size) => grid_size.value(i),
        };
        // PostGIS returns an empty input unchanged, checking geomB first (lwgeom_intersection_prec).
        if b.is_empty()? {
            builder.push_input(&geom_b, i)?;
            continue;
        }
        if a.is_empty()? {
            builder.push_input(&geom_a, i)?;
            continue;
        }
        let result = if grid_size < 0.0 {
            a.intersection(b)?
        } else {
            a.intersection_prec(b, grid_size)?
        };
        builder.push_geos(&result)?;
    }
    Ok(ColumnarValue::Array(builder.finish()))
}
```

A binary *predicate* moved to GEOS would prepare a scalar side once:
`if let Some(Some(b)) = geom_b.as_scalar() { let prepared = b.to_prepared_geom()?; … }`. That's
what PostGIS's prepared-geometry cache does for a constant argument.

### Template C: unary with a text options parameter (ST_Buffer)

```rust
fn buffer_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geom = GeosColumn::try_new(&args.args[0], &args.arg_fields[0])?;
    let radius = float64_arg(args.args.get(1), args.number_rows)?
        .expect("radius is required, enforced by the signature");
    // The third argument is either a parameter string or quad_segs (integer overload).
    let options = buffer_options_arg("ST_Buffer", args.args.get(2), args.number_rows)?;
    let mut builder = GeosGeometryBuilder::try_new(&args.return_field, args.number_rows)?;
    for i in 0..args.number_rows {
        let (Some(geom), false) = (geom.get(i), radius.is_null(i)) else {
            builder.push_null();
            continue;
        };
        let Some(style) = options.get(i)? else {
            builder.push_null();
            continue;
        };
        builder.push_geos(&geom.buffer_with_params(radius.value(i), &style.to_buffer_params()?)?)?;
    }
    Ok(ColumnarValue::Array(builder.finish()))
}
```

The parameter string is parsed per row. Parsing takes microseconds next to a buffer, and it
supports column-valued options without a cache.

### Template D: fixed return type (ST_MinimumClearance)

A unit struct with `new()`, `return_type` returning `DataType::Float64`, and a
`Float64Builder::with_capacity(args.number_rows)` in place of `GeosGeometryBuilder`. Everything
else is the same.

### Template E: PROJ (ST_Transform)

```rust
fn transform_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let from = proj_definition("ST_Transform", &input_crs(&args)?)?; // or the from_proj argument
    let to = proj_definition("ST_Transform", &return_crs(&args)?)?;
    let array = from_arrow_array(&args.args[0].to_array(args.number_rows)?, &args.arg_fields[0])?;
    // PostGIS returns the input unchanged when the source and target are the same.
    if from == to { /* return the input array with the return field's metadata */ }
    with_cached_proj(&from, &to, |proj| {
        let mut builder = GeometryBuilder::new(return_geometry_type(&args)?);
        downcast_geoarrow_array!(array, impl_transform, proj, &mut builder)?;
        Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
    })
}
```

- `proj::Proj` holds raw pointers, so it's neither `Send` nor `Sync`. `with_cached_proj` keeps a
  small thread-local map from `(from, to)` to `Proj`, because `proj_create_crs_to_crs` looks
  operations up in `proj.db` (milliseconds).
- `Proj::new_known_crs` applies `proj_normalize_for_visualization` (lon/lat order), as PostGIS
  does.
- `impl_transform` maps every coordinate with `proj.convert((x, y))`, keeps M unchanged, and returns
  `NotImplemented` for Z until the `proj` crate gets a 3D/inverse API. It reuses G1's
  coordinate-mapping helper (also needed by ST_Translate/ST_Affine) instead of converting to
  `geo`, which would lose Z/M.
- The target CRS goes into the return field, so the target SRID (or `to_proj`) must be a scalar.
  `return_field_from_args` reads `args.scalar_arguments` and returns `Plan` otherwise.

`proj_definition` maps GeoArrow CRS metadata to something `proj_create` accepts:

| `CrsType` | PROJ input |
|---|---|
| `AuthorityCode` (`EPSG:4326`, what geodatafusion writes today) | as is |
| `Srid` | `EPSG:<srid>`, falling back to `ESRI:<srid>`. The oracle's `spatial_ref_sys` has `srid = auth_srid` everywhere, with 6184 EPSG and 2315 ESRI codes that don't overlap, plus 900913. |
| `Projjson` | the JSON text (PROJ >= 6.2 reads PROJJSON) |
| `Wkt2_2019`, unknown | as is |
| none | error `ST_Transform: Input geometry has unknown (0) SRID`, as PostGIS |

The integer→CRS direction (`crs_from_srid`, used for the output of `ST_Transform(geom, srid)`) and
how SRIDs are represented are G6's. `ST_Transform(geom, to_proj text)` gives SRID 0 in PostGIS.
Representing that as a CRS the test renderer doesn't turn into an SRID keeps parity.

## 5. Dependencies

### Crate APIs (verified in `~/.cargo/registry/src`)

| API | Crate / feature | Used for |
|---|---|---|
| `Geometry::{create_point, create_line_string, create_linear_ring, create_polygon, create_multipoint, create_multiline_string, create_multipolygon, create_geometry_collection, create_empty_point, create_empty_polygon}` | `geos` 11.1.1 | `to_geos` |
| `CoordSeq::new_from_buffer(&[f64], usize, CoordType)` | `geos/v3_10_0` | `to_geos` |
| `WKBWriter::{new, set_output_dimension(CoordDimensions::ThreeD), write_wkb}` | `geos` (`FourD` needs `v3_12_0`; we want `ThreeD`) | `GeosGeometryBuilder` |
| `wkb::reader::read_wkb` | `wkb` 0.9 | `GeosGeometryBuilder` |
| `GeometryBuilder::{new, push_geometry, push_null, finish}` | `geoarrow-array` 0.8 | `GeosGeometryBuilder` |
| `downcast_geoarrow_array!(array, fn, args…)` | `geoarrow-array` 0.8 | `GeosColumn`, ST_Transform |
| `Signature::user_defined`, `ScalarUDFImpl::coerce_types`, `Signature::with_parameter_names` | `datafusion` 54.1 | R4 |
| `Proj::new_known_crs(from, to, None)`, `Proj::new(pipeline)`, `Proj::convert` | `proj` 0.31 (MSRV 1.85, PROJ >= 9.6.2) | ST_Transform, ST_TransformPipeline |

GEOS operations and the minimum GEOS version of each are in section 7. Not wrapped by `geos`
11.1.1 or 11.3.1: `GEOSLargestEmptyCircle_r` (3.9), `GEOSConstrainedDelaunayTriangulation_r`
(3.10), `GEOSPolygonHullSimplify_r` (3.11), `GEOSisValidDetail_r`, `GEOSCoverageIsValid_r` and
`GEOSCoverageSimplifyVW_r` (3.12), `GEOSCoverageClean_r` (3.14), `GEOSOrientPolygons_r` (3.12).

### PROJ crate choice

| Option | Parity | Build | Notes |
|---|---|---|---|
| **`proj` 0.31** (georust) | Same library as PostGIS | C++ PROJ >= 9.6.2 via pkg-config, otherwise a bundled source build (cmake, sqlite3). Needs `proj.db` at runtime. | 2D and forward only, `!Send`. Same org as `geo`/`geos`. **Recommended.** Upstream a 3D + inverse `trans_generic` wrapper. |
| `proj-sys` directly | Same | Same | Own unsafe wrapper (~150 lines) for 3D/inverse/database queries. Fallback if upstream is slow. |
| `proj4rs` 0.2 | proj.4 semantics, no EPSG database (via `crs-definitions`), no pipelines or WKT2 | Pure Rust | Datum shifts differ from PROJ 9. No parity. |
| `proj-core` 0.11 | Unknown | Pure Rust, embedded EPSG registry | Too new to depend on. Worth a look later for wasm builds. |

### Other groups

- **G6:** `coerce_args` (R4), `geometry_return_field` and CRS equality (R5), the SRID↔CRS
  mapping, `box2d` argument decoding (ST_ClipByBox2D), the record/struct return convention
  (MIC, LEC, IsValidDetail), and the geography type (deferred overloads).
- **G4:** WKT with untagged Z (`MULTILINESTRING((-29 -27 11, …))`, the last ST_LineMerge doc
  test), `ST_GeomFromEWKT`/`ST_AsEWKT` (ST_Difference, ST_SymDifference, ST_Transform), `ST_AsText`
  number formatting (every doc test).
- **G5:** the doc tests of ST_BuildArea, ST_OffsetCurve and ST_LargestEmptyCircle use `ST_Collect`,
  and MIC/LEC are called in `FROM` (table function). G5's ST_Union aggregate, ST_Polygonize,
  ST_MemUnion, ST_Coverage*, ST_Subdivide and clustering use the G3 bridge. The scalar
  `ST_Union(geomA, geomB[, gridSize])` and the aggregate `ST_Union(geom)` share a name. DataFusion
  resolves scalar functions before aggregates, so a registered scalar `st_union` would shadow the
  aggregate. This needs a joint decision.
- **G1:** the coordinate-mapping helper (ST_Transform). `ST_MakeLine`, `ST_Point`, `ST_Letters`,
  `ST_Segmentize`, `ST_Boundary`, `ST_ForceRHR`, `ST_SetSRID` appear in G3 doc tests.
- **G2:** the geo-vs-GEOS policy and the reassignments in section 1.

## 6. Phasing

| Batch | Content | Unlocks |
|---|---|---|
| **0. Infrastructure** | R2 features, R3 errors, R1 bridge (`udf/geos/util`), R4/R5 helpers (unless G6 has them), ST_LineMerge rewritten on the bridge (Z, NULL `directed`, array `directed`), `slt/geodatafusion/st_linemerge.slt`, R6 CI (static GEOS 3.14.1, feature-check job), README note on GEOS versions. Python (R7) once decided. | ST_LineMerge 5/5 after G4's WKT fix. Every later batch. |
| **1. Overlay** | ST_Intersection, ST_Difference, ST_SymDifference, ST_UnaryUnion, scalar ST_Union (after the G5 naming decision) | 7 doc tests (some need G4's EWKT functions) |
| **2. Simple unary** | ST_MakeValid (+ params), ST_ReducePrecision, ST_Normalize, ST_Node, ST_BuildArea, ST_MinimumClearance, ST_MinimumClearanceLine, ST_ConcaveHull and ST_SimplifyPreserveTopology (moved from G2) | 22 doc tests (ST_BuildArea needs ST_Collect, ST_SimplifyPreserveTopology needs ST_Buffer + ST_NPoints) |
| **3. Parameterised** | ST_Buffer (`BufferStyle`), ST_OffsetCurve (shares `BufferStyle`), ST_Snap, ST_SharedPaths, ST_VoronoiPolygons, ST_VoronoiLines, ST_DelaunayTriangles, ST_ClipByBox2D (needs G6 `box2d`) | 32 doc tests. ST_Buffer is the most-used GEOS function, but half its examples also need ST_Transform/ST_SetSRID/ST_ForceRHR. |
| **4. Validation and accessors** (if agreed with G1/G2) | ST_IsValid, ST_IsValidReason, ST_IsSimple, ST_IsRing on GEOS | 3+2+2 doc tests |
| **5. Upstream-blocked and records** | `geos` PR for LEC, constrained Delaunay, polygon hull, `isValidDetail` (and the coverage functions for G5). Then ST_TriangulatePolygon, ST_SimplifyPolygonHull, ST_IsValidDetail, ST_MaximumInscribedCircle, ST_LargestEmptyCircle (after the G6 struct convention and G5 table functions). | 11 doc tests |
| **6. PROJ** | `proj` feature, `udf/proj`, ST_Transform (4 overloads, 2D), ST_TransformPipeline (forward). Upstream 3D + inverse, then Z and ST_InverseTransformPipeline. postgis_srs* if a database API appears. | 7 doc tests, and unblocks the ST_Buffer/ST_Area examples that transform |
| **7. ST_Split** | Port PostGIS's `lwgeom_split` (line by point/multipoint/line/polygon boundary; polygon by line via union + polygonize) on GEOS operations | 3 doc tests (need ST_MakeLine, ST_Snap, ST_Buffer) |

Batches 1-3 are independent once batch 0 lands and can be done one function (or family) per PR,
as the style guide requires.

## 7. Per-function notes

All PostGIS functions here are `STRICT` (NULL in any argument gives NULL) except
ST_VoronoiPolygons/Lines, and all drop M and keep Z unless noted. "Empty → input" means the
input row is returned with `push_input`, keeping M. Doc tests are `passed/total` from
`parity.txt`. Sizes: S < half a day, M about a day, L several days.

| Function | `geos` API | Min GEOS | PostGIS gotchas | Size | Doc tests |
|---|---|---|---|---|---|
| ST_LineMerge | `Geometry::line_merge`, `line_merge_directed` (`v3_11_0`) | 3.11 | Empty → input. Non-lineal → `GEOMETRYCOLLECTION EMPTY`. Polygons pass through as rings. Z kept (broken today). | S | 4/5 |
| ST_Intersection | `intersection`, `intersection_prec` (`v3_9_0`) | 3.9 | `gridSize` default -1, < 0 means none. Empty geomB → geomB, then empty geomA → geomA. Mixed SRID error. Z interpolated (`POINT Z (1 0 1.5)`). | S | 0/3 |
| ST_Difference | `difference`, `difference_prec` | 3.9 | Empty geomA or geomB → geomA. | S | 0/2 |
| ST_SymDifference | `sym_difference`, `sym_difference_prec` | 3.9 | Empty geomA → geomB, empty geomB → geomA. | S | 0/2 |
| ST_Union (scalar) | `union`, `union_prec` | 3.9 | Name shared with the G5 aggregate. Empty → the other. PostGIS errors on some Z/2D mixes (`lwcollection_construct: mixed dimension`). | S | (aggregate example) |
| ST_UnaryUnion | `unary_union`, `unary_union_prec` | 3.9 | `gridSize` default -1. | S | — |
| ST_Node | `node` | 3.6 | PostGIS raises `Noding geometries of dimension != 1 is unsupported` itself. | S | 0/2 |
| ST_ClipByBox2D | `clip_by_rect(xmin, ymin, xmax, ymax)` | 3.6 | `box2d` argument (G6). Disjoint → empty (`POLYGON EMPTY`), Z kept. The result may be invalid (documented). | M | — |
| ST_Split | none (`GEOSSplit` is 3.15 and differs). Port of `lwgeom_split` on `intersection`/`difference`/`union`/`polygonize`. | 3.6 | Always returns a GEOMETRYCOLLECTION. Part order must match PostGIS. | L | 0/3 |
| ST_Buffer | `buffer_with_params(width, &BufferParams)` | 3.6 | Overloads: options text (default `''`) and `quad_segs` integer. Keys are case-sensitive and unknown keys error (`Invalid buffer parameter: ENDCAP`). Values: `endcap=round\|flat\|butt\|square`, `join=round\|mitre\|miter\|bevel`, `mitre_limit`/`miter_limit`, `quad_segs` parsed with `atoi` (`abc` → 0), `side=both\|left\|right` (single-sided, right negates the radius). Empty → `POLYGON EMPTY`. Negative radius on point/line → `POLYGON EMPTY`. NaN radius → GEOS error. Z dropped by GEOS. Geography/text overloads deferred. | M | 0/14 |
| ST_OffsetCurve | `offset_curve(width, quadsegs, join, mitre)` | 3.6 | Non-lineal → error (`input is not linear`). Empty → input. MultiLineStrings are offset per part and collected. Options: `quad_segs`, `join`, `mitre_limit` (shares `BufferStyle`). Result direction changed in GEOS 3.11. | M | 0/6 |
| ST_MakeValid | `make_valid`, `make_valid_with_params(&MakeValidParams)` (`v3_10_0`) | 3.10 | Params `method=linework\|structure`, `keepcollapsed=true\|false`. Unknown keys are ignored, a bad method errors. Empty is *not* passed through (`POLYGON M EMPTY` → `POLYGON EMPTY`). Collapsed lines → `POINT`. | M | 0/4 |
| ST_ReducePrecision | `set_precision(grid, Precision::ValidOutput)` | 3.9 | Empty → input (`POINT M EMPTY` kept). Z kept. | S | 0/5 |
| ST_Normalize | `Geometry::normalize(&mut self)` | 3.6 | Empty is not passed through (`POINT M EMPTY` → `POINT EMPTY`). Clone before normalising. | S | 0/1 |
| ST_Snap | `snap(other, tolerance)` | 3.6 | Empty → input. | S | 0/4 |
| ST_SharedPaths | `shared_paths(other)` | 3.6 | GEOS errors for non-lineal input pass through. | S | 0/2 |
| ST_BuildArea | `Geometry::build_area` (`v3_8_0`) | 3.8 | Returns NULL when no area is built. | S | 0/2 |
| ST_DelaunayTriangles | `delaunay_triangulation(tolerance, only_edges)` | 3.6 | `flags`: 0 → GC of polygons, 1 → MULTILINESTRING, 2 → TIN (`NotImplemented`). Empty → `GEOMETRYCOLLECTION EMPTY`. Z kept. | S | 0/3 |
| ST_VoronoiPolygons / ST_VoronoiLines | `voronoi(Option<&G>, tolerance, only_edges)` | 3.6 | Not STRICT: `extend_to` may be NULL (default envelope). NULL or negative tolerance → error `Tolerance must be a positive number.` A single point or empty → `GEOMETRYCOLLECTION EMPTY`. Polygon order depends on the GEOS version. | S | 0/2, 0/1 |
| ST_TriangulatePolygon | not wrapped (`GEOSConstrainedDelaunayTriangulation_r`) | 3.10 | Empty → `GEOMETRYCOLLECTION EMPTY`. | S after upstream | 0/3 |
| ST_SimplifyPolygonHull | not wrapped (`GEOSPolygonHullSimplify_r`) | 3.11 | `is_outer` default true. Doc tests need ST_Letters (G1). | S after upstream | 0/3 |
| ST_ConcaveHull | `concave_hull(ratio, allow_holes)` (`v3_11_0`) | 3.11 | `param_pctconvex`, `param_allow_holes` default false. Check what PostGIS 3.6 does for polygonal input before relying on `concave_hull` alone. | S | 0/3 |
| ST_SimplifyPreserveTopology | `Geometry::topology_preserve_simplify` | 3.6 | Replaces the `geo` VW version. | S | 0/3 |
| ST_MaximumInscribedCircle | `maximum_inscribed_circle(tolerance)` (`v3_9_0`) | 3.9 | Returns `(center, nearest, radius)` from GEOS's two-point line. PostGIS sets the tolerance to `max(width, height) / 1000`. Used in `FROM`. | M | 0/1 |
| ST_LargestEmptyCircle | not wrapped (`GEOSLargestEmptyCircle_r`) | 3.9 | Record. `tolerance` default 0, `boundary` default `POINT EMPTY`. Used in `FROM`. | M after upstream | 0/2 |
| ST_MinimumClearance | `minimum_clearance` → `f64` | 3.6 | `Infinity` when there's no clearance (single point). Fixed `Float64` return. | S | 0/1 |
| ST_MinimumClearanceLine | `minimum_clearance_line` | 3.6 | `LINESTRING EMPTY` when there's no clearance. | S | 0/1 |
| ST_IsValidDetail | not wrapped (`GEOSisValidDetail_r`) | 3.6 | Record `(valid, reason, location)`. `flags = 1` allows ESRI self-touching rings. | M after upstream | 0/2 |
| ST_IsValid, ST_IsValidReason (moved) | `is_valid`, `is_valid_reason`. The `flags` overloads need `isValidDetail`. | 3.6 | Reason format `Self-intersection[x y]`. | S | —, 1/3 |
| ST_IsSimple, ST_IsRing (moved) | `is_simple`, `is_ring` | 3.6 | — | S | 0/2, 0/2 |
| ST_Transform (PROJ) | `Proj::new_known_crs` + `convert` | PROJ 9.6.2 | Overloads `(geom, srid)`, `(geom, to_proj)`, `(geom, from_proj, to_proj)`, `(geom, from_proj, to_srid)`. Unknown SRID error. Same CRS → input. Z transformed in PostGIS (gap). M kept. `to_proj` output has SRID 0. Target must be a scalar. | L | 0/3 |
| ST_TransformPipeline (PROJ) | `Proj::new(pipeline)` + `convert` | PROJ 9.6.2 | `to_srid` default 0. Check axis order for EPSG operation URNs against the oracle. | M | 0/2 |
| ST_InverseTransformPipeline (PROJ) | — (needs inverse) | — | Blocked. | M | 0/2 |
| postgis_srs* (PROJ) | — (needs database API) | — | Blocked. | M | 0/1 each |

## 8. Testing

- **GEOS version.** Expected output was recorded with GEOS 3.14.1 (the oracle). GEOS output
  changes between minor versions (buffer vertices, Voronoi order, MakeValid, offset curve
  direction in 3.11), so the parity suite must run on 3.14.1. R6 does that with `geos-static`
  in `--all-features` and `geos-src` 0.2.4. Don't record or update parity with system GEOS. When
  the oracle image moves to a new GEOS, update `geos-src`, re-record (`cargo slt --complete`) and
  update `parity.txt` in one PR.
- **Hand-written `slt/geodatafusion/st_<function>.slt`** for each GEOS function covers: NULL in
  every argument (including parameters, since PostGIS is STRICT), EMPTY in every geometry argument
  including `… M EMPTY` and `… Z EMPTY` (pass-through vs not), Z kept, M dropped, ZM, every
  geometry type including collections, SRID propagation, the mixed-SRID error (once G6 lands),
  parameter defaults and invalid parameter strings (`query error`), GEOS exceptions
  (ST_SharedPaths on points), and a column-valued parameter (`FROM (VALUES …)`), which the old
  scalar-only code couldn't do.
- **Unit tests** cover only what SQL can't show: the return field is a `GeometryType` with the
  UDF's `coord_type` and the input CRS, a scalar geometry against an array (the converted-once
  path), and fixed return types. They sit inside the feature-gated module, so they only compile
  with GEOS.
- **Feature gating.** `cargo slt` always uses `--all-features`. GEOS functions don't exist without
  it, and their slt files would fail. The R6 `check-features` job builds without features and with
  the floor feature, so a wrong `#[cfg]` fails in CI rather than for a user.
- **Composite results** (MIC, LEC, IsValidDetail) need struct rendering in
  `tests/sqllogictests/render.rs` matching PostGIS's record text (`(0101…,0101…,0.99…)`), or
  queries that select fields. Coordinate with the harness owner.
- **PROJ.** Numeric output for EPSG projections is stable across PROJ versions well within the 12
  significant digits compared. Datum transformations depend on `proj.db` contents. Record with the
  oracle's PROJ (9.8.1) and expect bundled 9.6.2 builds to differ only in grid-based cases.
  Document the PROJ version next to the GEOS version.

## 9. Style guide amendments

1. **Layout, GEOS features.** Replace "Never use a bare `feature = "geos"`, which doesn't exist"
   with: "The `geos-3_11` feature is the floor and gates the whole `geos/` provider, its shared
   `util` module and the GEOS error variants. A function that needs a newer GEOS also gets
   `#[cfg(feature = "geos-3_x")]` on its `mod`, `pub use` and `register` line. Features chain
   (`geos-3_12` implies `geos-3_11`). There's no unversioned `geos` feature." Rationale: the
   current text is factually wrong, and the rule should say what to do.
2. **Layout, providers.** Add `proj/` (PROJ, behind `proj`). Allow a provider-level `util` module
   (`geos/util/`) for machinery used by every category. Today it says category-level only.
3. **Implementation, GEOS-backed functions** (new subsection): use `GeosColumn` and
   `GeosGeometryBuilder` with an explicit `for i in 0..args.number_rows` loop. Never call
   `Geom::to_wkb()` on results (it writes 2D). Z is kept and M dropped, as PostGIS does. Say so in
   the documentation ("This function drops the M coordinate."). Before each call, write the
   PostGIS empty/argument shortcuts from `lwgeom_geos.c` with a comment citing them.
4. **Inputs, parameters.** Replace "If only scalar values are supported, accept
   `ColumnarValue::Scalar` and return `NotImplemented` for arrays, as `line_merge.rs` does" with:
   "Read parameters per row with the shared argument helpers, so column values work. NULL in any
   argument gives NULL, because PostGIS functions are `STRICT`, unless the PostGIS function isn't
   (check `pg_proc.proisstrict`). Use the PostGIS default for a missing optional argument." The
   current rule points at the code that gets NULL wrong.
5. **Signatures.** Allow `Signature::user_defined` with `coerce_types` delegating to the shared
   `coerce_args` helper for UDFs with several geometry or optional arguments. Add `coerce_types`
   to the trait method order after `signature`.
6. **Documentation.** End the description of a function that needs more than the floor GEOS with
   "Requires GEOS 3.x or later.", and add the same note to the README table. (rustdoc already
   shows the `cfg` through `doc_cfg` on docs.rs.)
7. **Providers policy** (new principle under Layout): one implementation per SQL function, in the
   provider that matches how PostGIS computes it. Don't switch implementations by feature.
8. **Python bindings.** GEOS and PROJ functions are exempt until wheels can ship those libraries
   (or: are bound in `geodatafusion.geos` behind the Python crate's `geos` feature, depending on
   the decision).
9. **Tests.** Parity is recorded with the GEOS version of the oracle (bundled through
   `geos-static`). Unit tests don't repeat behaviour that the slt files cover.

## 10. Open questions

1. **GEOS floor:** keep `geos-3_11` as the only feature for now, or add `geos-3_10` so Ubuntu
   22.04 (GEOS 3.10.2) users get everything except ST_LineMerge(directed) and ST_ConcaveHull?
2. **Pinning:** is a `geos-static` feature in `--all-features`, with `geos-src` 0.2.4 /
   `geos-sys` 2.0.9 pinned in `Cargo.lock` (and Dependabot told to leave them), acceptable? It
   holds the `geos` crate at 11.1.x until the oracle moves to GEOS 3.15.
3. **Reassignments:** move ST_ConcaveHull, ST_SimplifyPreserveTopology, ST_IsValid and
   ST_IsValidReason (G2) and ST_IsSimple and ST_IsRing (G1) to GEOS? They'd disappear from builds
   without GEOS. ST_ConcaveHull and ST_SimplifyPreserveTopology currently compute a different
   function.
4. **ST_Union:** how should the scalar `ST_Union(geomA, geomB)` and the aggregate `ST_Union(geom)`
   coexist in DataFusion? Same question for ST_Collect, ST_MakeLine and ST_Polygonize (with G5).
5. **Unwrapped GEOS functions:** upstream PRs to `georust/geos` (preferred), or a small local
   unsafe FFI layer over `geos::sys` (needs our own context handle and WKB I/O, since `AsRaw` is
   private)?
6. **PROJ:** OK with the `proj` crate (C++ PROJ >= 9.6.2, source build on Ubuntu CI)? Should
   `proj` be in `--all-features` / `cargo slt`? Upstream 3D/inverse support, or a direct `proj-sys`
   wrapper?
7. **Python:** ship GEOS (and later PROJ) in wheels as shared libraries bundled by
   auditwheel/delocate, as Shapely does? Static linking of LGPL GEOS is the alternative to avoid.
8. **Records:** struct-returning scalar UDFs plus table functions for MIC/LEC/IsValidDetail,
   following a G6/G5 convention?
9. **Errors:** prefix GEOS errors with the SQL function name (via `DataFusionError::context`), or
   keep them `External` and transparent?
10. **Docs:** state "Requires GEOS 3.x or later." in SQL documentation descriptions, or only in the
    README and rustdoc?
