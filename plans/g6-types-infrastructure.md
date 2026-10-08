# G6: types, operators and shared infrastructure

G6 owns everything that isn't one function: the `geometry`/`geography`/`box2d`/`box3d` SQL
types and their casts, the SRID model, PostGIS's operators, and the scaffolding every other group
builds UDFs with (signatures, argument and return-field helpers, errors, documentation,
registration). The first batch is foundations for the other groups. The type and operator work
comes after it.

Everything below was checked against DataFusion 54.0.0, sqlparser 0.62.0 and geoarrow 0.8.0 in
`~/.cargo/registry`. Claims about planner behaviour were verified with a scratch crate (in the
session scratchpad, not committed) that registers a toy type planner, cast rewrite and operator
planner on DataFusion 54 and 55. Failure counts come from `cargo slt -v` at commit `d2eb234`:
527 of 582 records fail.

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### 1.1 Functions and features

| Item | Kind | Notes |
|---|---|---|
| `geometry` | SQL type | `TypePlanner` maps it to `Binary` + `geoarrow.wkb`, typmod SRID as CRS. |
| `geography` | SQL type | As `geometry`, plus `edges: "spherical"`, default SRID 4326. |
| `box2d`, `box3d` | SQL type | `geoarrow.box` XY / XYZ, what `Box2D`/`Box3D` already return. |
| `geometry(...)`, `geography(...)` | cast functions | PostGIS's own cast functions (`geometry(text)`, `geometry(bytea)`, `geometry(box2d)`, `geometry(geography)`, `geography(geometry)`, ...). Not in the inventory because the reference doesn't list them. |
| Box2D, Box3D | function (implemented) | Become the `::box2d`/`::box3d` cast functions as well. Bounds fixes are G1's R7. |
| Casts out of `geometry` | rewrite | `::text` → ST_AsHEXEWKB, `::bytea` → ST_AsEWKB (both G4), `::geography`, `::box2d`, `::box3d`. |
| ST_SRID, ST_SetSRID | function | The SRID model's two user-facing functions. |
| `&&`, `&&&`, `~`, `@`, `~=` | operator | Bounding-box predicates. The `box2df` and `gidx` rows in the inventory are PostGIS index types and collapse into the same functions (any geometry or box operand). |
| `<<`, `>>`, `&<`, `&>`, `<<\|`, `\|>>`, `&<\|`, `\|&>` | operator | Bounding-box position predicates. |
| `<->`, `<#>`, `<<->>` | operator | Distances: true 2D distance, box distance, n-D centroid distance. |
| `\|=\|` | operator | Planned as G1's ST_DistanceCPA. |
| `=` | operator | Left to DataFusion (bytewise equality of the storage). See 7. |
| Shared scaffolding | infrastructure | `src/util/`, `src/sql/`, errors, documentation, registration (sections 3 and 4). |

### 1.2 Reassigned

| Item | From → to | Why |
|---|---|---|
| `geometry_dump` | G6 → G5 | Only the dump functions produce it; G5 defines the struct layout next to them. G6 adds no SQL type name for it. |
| postgis_srs, postgis_srs_all, postgis_srs_codes, postgis_srs_search | G1 proposed G6 → G5 | Table functions (the UDF kind decides the group, per the README), with data from G3's PROJ database. |
| ST_EstimatedExtent | G1 proposed G6 → won't do | Reads planner statistics for a named table. A DataFusion UDF can't reach the catalog, and statistics aren't a box. Document as unsupported. |
| ST_DistanceCPA (behind `\|=\|`) | stays G1 | G6 only wires the operator to it. |
| ST_GeomFromEWKT/EWKB, ST_AsEWKT/EWKB/HEXEWKB | stay G4 | G6 provides the SRID helpers they need. These are the biggest single blockers in the suite (see 6). |

### 1.3 What DataFusion 54 can't do

| Gap | Effect | Unblocked by |
|---|---|---|
| `Cast` drops the target field's metadata. `cast_output_field` copies the *source* metadata (`datafusion-expr-54.0.0/src/expr_schema.rs:74-88`, used at :598). | At SQL-planning time `'POINT(1 2)'::geometry` is a plain `Binary`. `VALUES ('...'::geometry)` loses the type and CRS for good (verified). The operator planner can't tell from metadata that a cast literal is a geometry. Casting a geometry to another type keeps the `geoarrow.*` tag on the wrong storage. | DataFusion 55 (`expr_schema.rs:87-117` uses the target field's metadata; verified: VALUES, subqueries and the operator planner all see the extension). The branch is on 55.1 (D17). A cast rewrite that changes the field, such as a CRS read from an `'SRID=n;...'` literal, doesn't reach a `VALUES` schema, which is fixed when the SQL is planned; an analyzer rule that recomputes it works (verified on 55.1). |
| INSERT casts values to the table schema with a type-only cast. | `INSERT INTO t VALUES (1, 'POINT(1 2)')` into a `geometry` column stores the text bytes as "WKB" (verified on 54 and 55). `INSERT ... SELECT '...'::geometry` works. | A custom analyzer rule that inspects `Dml` targets, or upstream assignment-cast hooks. Deferred. |
| One `TypePlanner` per session, set only through `SessionStateBuilder::with_type_planner` (`datafusion-54.0.0/src/execution/session_state.rs:1295`). | `geodatafusion::register(&SessionContext)` can't add the types. Users build their session with `GeoTypePlanner`. | Upstream: a list of type planners, like expression planners. |
| Geometric operators are tokenized only by `PostgreSqlDialect`/`RedshiftSqlDialect` (`sqlparser-0.62.0/src/tokenizer.rs:1547-1790`, `parser/mod.rs:3787-3867`). | The default Generic dialect parses only `&&`, `~`, `<<`, `>>`. Users set `datafusion.sql_parser.dialect = 'PostgreSQL'`. | Nothing needed beyond configuration. Switching the harness loses no record (all 582 parse at least as well; 16 more parse). |
| No extension-aware signatures: `TypeSignatureClass` can't name a GeoArrow type, and coercion sees only `DataType`. | Geometry arguments are validated against a `DataType` list, so a plain `Binary` counts as WKB and a plain `Utf8` as WKT. Geometry/geography overloads can't be told apart by the signature. | Upstream logical types / extension type registry. |
| Typed literals of custom types: `geometry 'POINT(1 2)'` parses `geometry` as a column. | Unsupported syntax (rare in the docs). | sqlparser. |
| Composite field access `(ST_Dump(g)).geom` (4 records). | Parse error. | sqlparser/DataFusion; G5's concern. |
| `datafusion-ffi` 54 has no FFI for expression/type planners or function rewrites. | Python users (via datafusion-python) get the functions but not the types, casts or operators. | Upstream FFI. |
| GeoArrow has no curve or surface types. | 57 records in 38 files use CIRCULARSTRING, CURVEPOLYGON, TIN, ... and fail at parse. | geoarrow/wkt/wkb. Out of scope. |
| Per-value SRIDs. | One CRS per column (by design, section 3.4). | Not planned. |
| B-tree operators (`<`, `>`, ORDER BY/GROUP BY on geometry). | DataFusion sorts the storage bytes, PostGIS sorts by a space-filling curve. | Not planned; document. |

## 2. Existing basis

| Location | What | Issues |
|---|---|---|
| `src/lib.rs:13-35` | `register(&SessionContext)`, one call per category | Registers UDFs only. No planners, rewrites or types. |
| `src/data_types.rs:10-87` | `any_geometry_type()`: 70 `DataType`s (7 native types × 4 dims × 2 coord types, Geometry, Box, WKB, WKT) | Fine as a list, but `Signature::uniform(n, ..)` makes all `n` arguments the same type, so two-geometry functions fell back to `Signature::any` (`geo/measurement/distance.rs:28`, `geo/processing/simplify.rs:27,86,145`, `geo/relationships/topological/relate.rs:30`). Error messages list all 70 types. |
| `src/data_types.rs:89-130` | `any_single_geometry_type_input()`, `any_point_type_input(n)` | No helpers for numeric/SRID/text arguments, so every file builds its own (`geos/processing/line_merge.rs:22-30` expands 140 `Exact` variants). |
| `src/error.rs:1` | Module doc names `GeoArrowError` | Should be `GeoDataFusionError`. |
| `src/error.rs:22,39` | `#[cfg(feature = "geos")]` | Works only because Cargo creates an implicit `geos` feature for the optional dependency (`cargo metadata`: `geos: ["dep:geos"]`) and `geos-3_11 = ["geos/v3_11_0"]` turns it on. It's a public no-op feature, and the style guide forbids the bare name. |
| 32 UDFs, e.g. `constructors/point.rs:66` | `Err(DataFusionError::Internal("return_type".to_string()))` | DataFusion's convention is `internal_err!("return_field_from_args should be called instead")` (`datafusion-functions-54.0.0/src/core/arrow_cast.rs:125`, `core/getfield.rs:314`). |
| 25 other sites | Hand-built `DataFusionError::Internal/NotImplemented` and `unreachable!()` | User errors reported as `Internal` (`point.rs:78`); `unreachable!()` on input types (`io/wkt.rs:124`, `io/wkb.rs:121`). |
| 58 UDFs | `static DOCUMENTATION: OnceLock<Documentation>` + `Documentation::builder(DOC_SECTION_OTHER, ..)` | Every function is in "Other Functions". `datafusion-functions` uses `#[user_doc]` (196 uses, 1 builder). Argument "descriptions" are type names; syntax examples drift (`point.rs:190` documents ST_PointZ as `ST_Point(..)`). |
| `constructors/point.rs:73-82` (and :165, :258, :353) | SRID from `scalar_arguments` → `Crs::from_authority_code("EPSG:n")` | Copied four times. `srid_val.unwrap()` panics on `ST_Point(1, 2, NULL)`. Only `Int64` is accepted. `Internal` for a user error. |
| `io/wkt.rs:56-68`, `io/wkb.rs:46-60` | ST_AsText/ST_AsBinary return `Utf8`/`Binary` tagged `geoarrow.wkt`/`geoarrow.wkb` | They return `text`/`bytea` in PostGIS. The tag makes ST_AsBinary render as a geometry and forced a special case in the harness (`datafusion_engine.rs:94-96`). G4 R1 removes it; G6 makes it a rule (3.4). |
| `bounding_box/box.rs:17-131` | Box2D/Box3D: `geoarrow.box` with the input CRS | Good basis for the box types. `box_impl` duplicates ST_Envelope (comment at :125). |
| `geo/relationships/topological/intersects.rs` | A second `Intersects` | Dead: `topological/mod.rs` only has `mod relate`. |
| `Cargo.toml:27` | `datafusion = { version = "54", default-features = false }` | The library has no `sql` feature, so `TypePlanner` and the sqlparser-typed `RawBinaryExpr::op` (`datafusion-expr-54.0.0/src/planner.rs:288-295`, `cfg(feature = "sql")`) aren't available to it. |
| `tests/sqllogictests/datafusion_engine.rs:185-353` | The `::geometry` literal shim | String-level rewrite of literal casts only. `geom::geometry`, `x::geography` on expressions and `::text` aren't handled. |
| `datafusion_engine.rs:79-90`, `:138-148` | Renders from `batch.schema()`; SRID from `EPSG:n` or `srid` CRS | On DataFusion 54, `MemTable` scan batches drop field metadata (verified), so a geometry column read from a table renders as bytes. |

## 3. Refactoring assessment

### R1. Crate-wide helper modules (do, first)

`data_types.rs` holds signatures only, and three other plans propose more helpers for it (G2 and
G3: `coerce_args`, argument readers, return fields; G4: SRID helpers; G1: kernels in `src/util/`).
One crate-private `src/util/` module, owned by G6, collects them:

```
src/util/
├── mod.rs
├── signature.rs   any_geometry_type(), single_geometry(), Arg, coerce_args()   (was data_types.rs)
├── args.rs        per-row argument readers, constant (planning-time) readers
├── field.rs       geometry_array(), input_metadata(), common_metadata(), geometry_return_field(), is_geography()
├── srid.rs        SRID ↔ CRS
└── (G1: kernel.rs, builder.rs, ordinates.rs, owned.rs)
```

Helpers that only produce DataFusion errors return `datafusion::error::Result`, so DataFusion's
error macros work in them unchanged (R3). Effort M (moving and migrating the 4
`Signature::any` users and `line_merge.rs`), risk low.

### R2. Signatures: `user_defined` plus a shared `coerce_args` (do)

Options considered:

| Option | Plan-time type errors | Several geometry arguments | Keeps constant SRIDs literal | DataFusion-native |
|---|---|---|---|---|
| A. `OneOf` of `Exact` over `any_geometry_type()` × overloads (G1 R4) | yes | no: 70 × 70 variants for two geometries, and errors list every candidate | only for `Int64` | yes |
| B. `Signature::coercible` with `TypeSignatureClass::Any` for geometry | no (any type passes; errors at execution) | yes | yes (`TypeSignatureClass::Integer` exact) | yes |
| C. `Signature::user_defined` + `coerce_types` → shared `coerce_args` (G2, G3) | yes | yes | yes, with a dedicated `Srid` kind | yes (`coalesce`, `greatest`, `nvl2` do this) |

Recommendation: **C** for every function with a non-geometry argument or more than one
geometry; the shared `single_geometry()` static (`Uniform(1, any_geometry_type())`, today's
`any_single_geometry_type_input()`) for the many one-geometry functions. B was verified to work
(`Coercion::new_exact(TypeSignatureClass::Integer)` kept `Int64(4326)` as a literal through
every planning pass) but gives up planning-time checks, which is what the style guide bans
`Signature::any` for.

The `Arg` kinds are PostGIS's SQL types, so a file's overload table reads like the PostGIS
synopsis:

```rust
// Before (geo/processing/simplify.rs:27): no type checking, runtime errors.
signature: Signature::any(2, Volatility::Immutable),

// After
/// PostGIS: ST_Simplify(geometry geom, float tolerance, boolean preserveCollapsed = false).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "tolerance", "preserveCollapsed"])
        .expect("parameter names are valid for a user-defined signature")
});

impl ScalarUDFImpl for Simplify {
    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }
}
```

Two facts drive the `Srid` kind. `ReturnFieldArgs::scalar_arguments` only holds bare
`Expr::Literal`s (`datafusion-expr-54.0.0/src/expr_schema.rs:583-589`), and coercion wraps a
literal in a `Cast` that isn't folded until the optimizer runs. Verified: a `user_defined`
function that coerces `Int64` to `Float64` sees `scalar_arguments = [None]` in one of its
`return_field_from_args` calls. So an argument that sets the output CRS must keep whatever
integer type it arrives as. `with_parameter_names` is accepted for `user_defined`
(`datafusion-expr-common-54.0.0/src/signature.rs:1477-1486`) and named arguments
(`srid => 4326`, used by the doc tests) work (verified). Effort S for the helper, M to migrate,
risk low.

### R3. Errors: DataFusion's macros, honest variants (do)

- In functions returning `datafusion::error::Result`: `plan_err!`, `exec_err!`, `not_impl_err!`,
  `internal_err!` from `datafusion::common`, as `datafusion-functions` does.
- In `_impl` functions returning `GeoDataFusionResult` (needed because `GeoArrowError`,
  `geos::Error` and friends can't get a `From` into `DataFusionError` under the orphan rule):
  `return Err(exec_datafusion_err!("...").into());`.
- `return_type` of a UDF with `return_field_from_args`:
  `internal_err!("return_field_from_args should be called instead")`, DataFusion's wording.
- Messages start with `self.name()`, as DataFusion's do (`"{} requires ...", self.name()`). The
  harness doesn't compare error text (PostGIS errors are recorded as `query error db error`), so
  PostGIS-cased names buy nothing and invite typos.
- `error.rs`: fix the module doc; gate the GEOS variant on `geos-3_11` (the lowest GEOS feature,
  which every higher one implies); declare the dependency with `dep:geos` so the implicit `geos`
  feature disappears:

```toml
[features]
default = ["sql"]
# Enables the geometry/geography/box2d/box3d SQL types, casts and operators.
sql = ["datafusion/sql"]
geos-3_11 = ["dep:geos", "geos/v3_11_0"]
```

Effort S, risk none (the `geos` feature removal is technically breaking, but it never did
anything on its own).

### R4. Documentation with `#[user_doc]` and PostGIS chapter sections (do)

`datafusion-functions` documents every UDF with `datafusion_macros::user_doc`
(`datafusion-macros-54.0.0/src/user_doc.rs`). It generates a `fn doc(&self)` backed by a
`LazyLock`, and `documentation()` returns `self.doc()`. A label that isn't one of DataFusion's
constants is accepted with `include: true` (`user_doc.rs:197-205`), so sections can be PostGIS's
reference chapters.

```rust
// Before (geo/measurement/area.rs:31, 50-60)
static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();
...
    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns the area of a polygonal geometry.",
                "ST_Area(geom)",
            )
            .with_argument("geom", "geometry")
            .build()
        }))
    }

// After
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the area of a polygonal geometry.",
    syntax_example = "ST_Area(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Area;
...
    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
```

Costs: the macro expands to `datafusion_doc::...` paths, so the crate needs `datafusion-doc` and
`datafusion-macros` as direct dependencies (workspace, same version as `datafusion`). One
`#[user_doc]` per struct, so a file with several UDFs has several attributes (it already has
several statics). A unit test in `lib.rs` keeps it honest: every UDF registered by `register` has
documentation, its section label is in the chapter list (section 9), and its syntax example starts
with its name (case-insensitive) or, for operators, uses `alternative_syntax`. Effort M (58
mechanical edits, best done in one PR before the groups add ~250 more), risk low. G1 deferred this
decision to G6 (G1 R13).

### R5. One type model (do)

- **A field with a `geoarrow.*` extension is a geometry** (or geography, or box). Plain `Utf8` is
  `text` and plain `Binary` is `bytea`. ST_As* outputs are plain (G4 R1). As inputs, plain
  strings and binaries are still accepted as geometry, like PostgreSQL's implicit `text` and
  `bytea` casts. They go through `util::field::geometry_array`, which parses EWKT/HEXEWKB
  (text) and EWKB (binary) with G4's parsers, so `ST_Area('SRID=4326;POLYGON(...)')` parses like
  `'...'::geometry` would.
- **Geography** is any geometry field whose GeoArrow metadata has `edges: "spherical"`
  (`geoarrow-schema-0.8.0/src/edges.rs`). PostGIS geography edges are great-circle arcs, which is
  exactly that value. Spheroidal measurement is a per-function choice (`use_spheroid`), not a type
  property.
- **The `geometry` SQL type is WKB** (`Binary` + `geoarrow.wkb`). It holds any mix of types and
  dimensions per row, as a PostGIS column does. Functions keep returning native GeoArrow types
  with the UDF's `coord_type` (the status quo, and G4's recommendation for input functions). The
  cost: `UNION`/`CASE`/`COALESCE` over a WKB and a native geometry fail type coercion unless one
  side is cast with `::geometry`. See open question 1.
- **SRIDs** live in the field's CRS and only `util::srid` converts between the two (3.4).

Effort S as a rule. Its consequences are in G4 (outputs) and the harness.

### R6. The `sql` module: types, casts and operators (do, after R1-R5)

Behind the new `sql` feature (default on), because `TypePlanner` and the sqlparser-typed
`RawBinaryExpr::op` need `datafusion/sql`:

```
src/sql/
├── mod.rs        pub use; register(ctx) adds the operator planner and the cast rewrite
├── types.rs      pub struct GeoTypePlanner
├── operators.rs  pub struct GeoExprPlanner
└── casts.rs      struct GeoCastRewrite (FunctionRewrite)
src/udf/native/
├── types/        Geometry (geometry(...)), Geography (geography(...)) cast functions
├── srs/          SRID (ST_SRID), SetSRID (ST_SetSRID)
└── operators/    bounding-box predicate and distance functions behind the operators
```

**Types.** `GeoTypePlanner::plan_type_field` maps `DataType::Custom(name, modifiers)` (the last
identifier of `name`, case-insensitively) to a field, and passes anything else to an optional
fallback planner, since a session has only one:

| SQL | Field |
|---|---|
| `geometry`, `geometry(Geometry)` | `Binary`, `geoarrow.wkb`, no CRS |
| `geometry(<type>[Z\|M\|ZM], srid)` | as above, CRS from `srid_to_crs(srid)`. The subtype isn't enforced (open question 10). |
| `geography`, `geography(<type>[, srid])` | `Binary`, `geoarrow.wkb`, CRS of `srid` (default 4326), `edges: "spherical"` |
| `box2d` / `box3d` | `BoxType::new(XY/XYZ)` |

Verified on DataFusion 54: casts, `CREATE TABLE t (geom geometry(Point, 4326))` (the table schema
keeps the extension and CRS) and `NULL::geometry` all reach the planner.

**Casts.** The type planner only names the target. A `FunctionRewrite`
(`datafusion-expr-54.0.0/src/expr_rewriter/mod.rs:51`) registered with `register_function_rewrite`
runs before type coercion (`datafusion-optimizer-54.0.0/src/analyzer/mod.rs:134-150`) and turns
casts into calls:

| Cast | Becomes |
|---|---|
| `x::geometry[(...)]` | `geometry(x)`, an instance carrying the target field |
| `x::geography[(...)]` | `geography(x)` |
| `x::box2d`, `x::box3d` | `box2d(x)`, `box3d(x)` (the existing UDFs) |
| `geom::text` (`Utf8`, `Utf8View`, `LargeUtf8`) | `st_ashexewkb(geom)` (PostGIS's `text(geometry)` is hex EWKB) |
| `geom::bytea` | `st_asewkb(geom)` |
| geometry to any other type | plan error |

```rust
impl FunctionRewrite for GeoCastRewrite {
    fn name(&self) -> &str {
        "geodatafusion_casts"
    }

    fn rewrite(&self, expr: Expr, schema: &DFSchema, _: &ConfigOptions) -> Result<Transformed<Expr>> {
        let Expr::Cast(Cast { expr: input, field: target }) = &expr else {
            return Ok(Transformed::no(expr));
        };
        if let Some(kind) = SpatialKind::of(target) {
            return Ok(Transformed::yes(cast_to(kind, target, *input.clone())));
        }
        let (_, source) = input.to_field(schema)?;
        if SpatialKind::of(&source).is_some() {
            return cast_from(&source, target, *input.clone()).map(Transformed::yes);
        }
        Ok(Transformed::no(expr))
    }
}
```

`geometry(x)` accepts text (WKT, EWKT, HEXEWKB), `bytea` (EWKB), any GeoArrow geometry, a box
(its polygon) and geography, and returns exactly the target field, so the plan stays consistent
whether or not DataFusion recomputes a schema. An SRID in a literal sets the CRS when the target
has none (`'SRID=4326;POINT(1 2)'::geometry`); an SRID that contradicts the target's is an
execution error, as in PostGIS ("Geometry SRID (3857) does not match column SRID (4326)").
Parsing is G4's EWKT/EWKB code.

**Operators.** `GeoExprPlanner::plan_binary_op` (`datafusion-expr-54.0.0/src/planner.rs:155`)
gets the raw sqlparser operator before DataFusion maps it
(`datafusion-sql-54.0.0/src/expr/mod.rs:123-150`), and plans a call of the PostGIS backing
function:

| Operator | sqlparser (`PostgreSqlDialect`) | Function |
|---|---|---|
| `&&` | `PGOverlap` | `geometry_overlaps` |
| `&&&` | `Custom("&&&")` | `geometry_overlaps_nd` |
| `@` | `At` | `geometry_within` |
| `~=` | `TildeEq` | `geometry_same` |
| `&<`, `&>` | `AndLt`, `AndGt` | `geometry_overleft`, `geometry_overright` |
| `<<\|`, `\|>>` | `LtLtPipe`, `PipeGtGt` | `geometry_below`, `geometry_above` |
| `&<\|`, `\|&>` | `AndLtPipe`, `PipeAndGt` | `geometry_overbelow`, `geometry_overabove` |
| `<->` | `LtDashGt` | `geometry_distance_centroid` |
| `<#>` | `Custom("<#>")` | `geometry_distance_box` |
| `<<->>` | `Custom("<<->>")` | `geometry_distance_centroid_nd` |
| `\|=\|` | `Custom("\|=\|")` | `st_distancecpa` (G1) |
| `~` | `PGRegexMatch` | `geometry_contains`, only for spatial operands |
| `<<`, `>>` | `PGBitwiseShiftLeft/Right` | `geometry_left`, `geometry_right`, only for spatial operands |

All tokens and operators above were verified to reach the planner. DataFusion has no meaning for
the first group, so it's always spatial. `~`, `<<` and `>>` are spatial when an operand has a
`geoarrow.*` extension or a storage type the built-in operator rejects anyway (`Binary*`,
`Struct`, `Union`, lists). That second test is what makes it work on DataFusion 54, where a cast
literal has lost its extension at SQL-planning time. The functions are registered under their
PostGIS names too, so `geometry_overlaps(a, b)` works in the Generic dialect.

**Registration.**

```rust
// User code
let state = SessionStateBuilder::new()
    .with_default_features()
    .with_type_planner(Arc::new(GeoTypePlanner::new()))
    .build();
let ctx = SessionContext::new_with_state(state);
ctx.sql("SET datafusion.sql_parser.dialect = 'PostgreSQL'").await?; // for the operators
geodatafusion::register(&ctx); // UDFs, operator planner, cast rewrite
```

Effort L. Risk medium: it's new public API, and the DataFusion 54 workarounds (operand storage
test, lost VALUES metadata) go away with 55.

### R7. A macro or trait for `ScalarUDFImpl` (don't)

- A blanket `impl<T: GeoUdf> ScalarUDFImpl for T` breaks the orphan rule (foreign trait, uncovered
  type parameter). A wrapper `GeoUdf<T>(T)` would make every public type a wrapper, in Rust and
  Python.
- A `macro_rules!` generating the impl (like `relate.rs:20-82`) hides the shape the style guide
  wants every file to share, and DataFusion doesn't do it (only `make_udf_function!` singletons).
  G1 reached the same conclusion (R12).

What actually cuts the ~60 lines: `#[user_doc]` (-8 to -12 per UDF), `coerce_args` and the
argument readers (-10 to -40 in multi-argument functions), `scalar_srid` (-10 per SRID argument),
`geometry_return_field` (-5). Keep a `macro_rules!` only for a family of near-identical
functions in one file, as the topological predicates are.

### R8. Smaller fixes (do, with R1)

- Return fields are named `self.name()` and nullable, as DataFusion's default
  `return_field_from_args` does (`datafusion-expr-54.0.0/src/udf.rs:677-686`), instead of `""`.
- Delete `geo/relationships/topological/intersects.rs` (dead), coordinated with G2.
- `udf/native/io` and G4 stop using `unreachable!()` for input types the signature allows.

### R9. Formatting extension types for `DataFrame::show` (later)

DataFusion 54 renders extension columns through its `ExtensionTypeRegistry`
(`datafusion-54.0.0/src/dataframe/mod.rs:1523`). Registering a `DFExtensionType` for the
`geoarrow.*` names makes `df.show()` print EWKT instead of bytes or structs. `register` can add it
through `Session::extension_type_registry()`. Effort S, risk none.

## 4. Canonical templates and helpers

All in `rust/geodatafusion/src/util/` (crate-private) unless stated. Other groups use these and
don't add parallel versions; a missing helper is added here.

### 4.1 `util/signature.rs`

```rust
/// Every GeoArrow type a geometry argument accepts: native types, boxes, WKB and WKT.
pub(crate) fn any_geometry_type() -> Vec<DataType>;

/// The signature of a function with one geometry argument.
pub(crate) fn single_geometry() -> &'static Signature;

/// A PostGIS argument type, for [`coerce_args`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arg {
    /// `geometry`, `geography`, `box2d` or `box3d`: any of [`any_geometry_type`], kept as is so
    /// the field metadata survives. `Null` becomes `Binary` (a NULL WKB).
    Geometry,
    /// `float8`: numeric types and `Null` become `Float64`.
    Float,
    /// `integer`: integer types and `Null` become `Int32`.
    Integer,
    /// An `integer` SRID: integer types are kept as is, so a constant stays a literal for
    /// `return_field_from_args`. `Null` becomes `Int32`.
    Srid,
    /// `boolean`: `Null` becomes `Boolean`.
    Boolean,
    /// `text`: string types are kept as is. `Null` becomes `Utf8`.
    Text,
}

/// `coerce_types` for a `Signature::user_defined` UDF: the coerced types of the first overload
/// in `overloads` that `arg_types` match, otherwise a plan error naming `name`.
pub(crate) fn coerce_args(name: &str, arg_types: &[DataType], overloads: &[&[Arg]])
    -> Result<Vec<DataType>>;
```

`any_point_type_input(n)` stays for the point-only accessors until G1 R4 retires it.

### 4.2 `util/args.rs`

```rust
/// Argument `index` cast to `Float64`, one value per row (scalars are broadcast).
pub(crate) fn float_arg(args: &ScalarFunctionArgs, index: usize) -> Result<Float64Array>;
/// Like [`float_arg`], but `default` for every row if the argument is absent.
pub(crate) fn optional_float_arg(args: &ScalarFunctionArgs, index: usize, default: f64)
    -> Result<Float64Array>;
// int_arg/optional_int_arg (Int32Array), bool_arg/optional_bool_arg, text_arg/optional_text_arg
// (StringArray) likewise.

/// The constant SRID argument `index` when planning, clamped like PostGIS. `None` if the
/// argument is absent or NULL (the function then returns NULL). A non-constant SRID is a plan
/// error: the SRID becomes the output column's CRS.
pub(crate) fn scalar_srid(name: &str, args: &ReturnFieldArgs, index: usize) -> Result<Option<i32>>;

/// The constant text argument `index` when planning (ST_Transform's `to_proj`).
pub(crate) fn scalar_text<'a>(name: &str, args: &'a ReturnFieldArgs, index: usize)
    -> Result<Option<&'a str>>;
```

Array arguments use `datafusion::common::utils::take_function_args` instead of indexing or
`next().unwrap()`.

### 4.3 `util/field.rs`

```rust
/// Decodes geometry argument `index`. Plain strings and binaries are parsed as EWKT/HEXEWKB and
/// EWKB (PostgreSQL's implicit casts); `Null` gives an all-NULL array.
pub(crate) fn geometry_array(args: &ScalarFunctionArgs, index: usize)
    -> GeoDataFusionResult<Arc<dyn GeoArrowArray>>;

/// The GeoArrow metadata (CRS and edges) of a field. Default for untagged fields.
pub(crate) fn input_metadata(field: &Field) -> Result<Arc<Metadata>>;

/// The metadata shared by the geometry arguments at `indices`. Different SRIDs are an error,
/// as in PostGIS ("Operation on mixed SRID geometries"), and so is geometry mixed with
/// geography.
pub(crate) fn common_metadata(name: &str, fields: &[FieldRef], indices: &[usize])
    -> Result<Arc<Metadata>>;

/// The return field of a UDF returning `output` (its metadata is replaced by the
/// [`common_metadata`] of `geometry_args`), named after the UDF.
pub(crate) fn geometry_return_field(
    name: &str,
    args: &ReturnFieldArgs,
    geometry_args: &[usize],
    output: GeoArrowType,
) -> Result<FieldRef>;

/// Whether a field holds geography: GeoArrow `edges: "spherical"`.
pub(crate) fn is_geography(field: &Field) -> bool;
```

When the SRID check runs: functions returning geometry call `geometry_return_field` from
`return_field_from_args` (planning). Fixed-return functions (ST_Distance, ST_Intersects) keep
`return_type` and call `common_metadata(self.name(), &args.arg_fields, &[0, 1])?` at the top of
their `_impl`, at execution, which is also when PostGIS checks. Geography dispatch follows the
same split (answers G2's question 8).

### 4.4 `util/srid.rs`

```rust
/// PostGIS's "unknown" SRID, stored as no CRS.
pub(crate) const SRID_UNKNOWN: i32 = 0;

/// PostGIS's `clamp_srid`: SRIDs <= 0 become 0, SRIDs above 999999 are folded into the
/// reserved range, as PostGIS does with a NOTICE.
pub(crate) fn clamp_srid(srid: i64) -> i32;

/// The GeoArrow CRS for a PostGIS SRID: none for 0, `ESRI:n` for PostGIS's ESRI codes,
/// otherwise `EPSG:n`.
pub(crate) fn srid_to_crs(srid: i32) -> Crs;

/// The PostGIS SRID of a GeoArrow CRS: 0 for none; the code of an `EPSG:`/`ESRI:` authority
/// code, a `srid` CRS or a PROJJSON/WKT2 `id`; 4326 for `OGC:CRS84`. `None` for a CRS without
/// one.
pub(crate) fn crs_to_srid(crs: &Crs) -> Option<i32>;
```

The ESRI codes are a sorted `&[i32]` generated from PostGIS's `spatial_ref_sys` (2315 rows;
6184 more are EPSG with `srid = auth_srid`, plus `900913`). Nobody else formats `"EPSG:{n}"`.

### 4.5 Template: two geometries, fixed return type

```rust
use std::sync::{Arc, LazyLock};

use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Distance(geometry g1, geometry g2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the distance between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the minimum 2D Cartesian distance between two geometries, in projected units.",
    syntax_example = "ST_Distance(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_3ddistance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance;

impl Distance {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Distance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Distance {
    fn name(&self) -> &str {
        "st_distance"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args.arg_fields, &[0, 1])?;
    let left = geometry_array(&args, 0)?;
    let right = geometry_array(&args, 1)?;
    // ...
}
```

### 4.6 Template: SRID argument, geometry return (ST_SetSRID)

```rust
fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
    let input = GeoArrowType::from_arrow_field(&args.arg_fields[0])
        .map_err(|e| plan_datafusion_err!("{}: {e}", self.name()))?;
    let edges = input_metadata(&args.arg_fields[0])?.edges();
    let crs = scalar_srid(self.name(), &args, 1)?.map(srid_to_crs).unwrap_or_default();
    let output = input.with_metadata(Arc::new(Metadata::new(crs, edges)));
    Ok(Arc::new(output.to_field(self.name(), true)))
}

fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
    // The storage is unchanged; only the field's CRS differs. A NULL SRID gives NULL.
    ...
}
```

Also part of the template set: the operator functions document their operator with
`alternative_syntax = "geomA && geomB"`, and the trait methods appear in the order `name`,
`aliases`, `signature`, `return_type`, `return_field_from_args`, `coerce_types`,
`invoke_with_args`, `simplify`, `documentation`.

## 5. Dependencies

### 5.1 Crate APIs relied on (verified)

| Crate | Version | API |
|---|---|---|
| datafusion-expr | 54.0.0 | `planner::{TypePlanner::plan_type_field, ExprPlanner::plan_binary_op, RawBinaryExpr, PlannerResult}` (`planner.rs:152-163, 288-295, 439-466`); `expr_rewriter::FunctionRewrite` (`expr_rewriter/mod.rs:51-65`); `ScalarUDFImpl::{return_field_from_args, coerce_types, simplify, documentation}`; `ReturnFieldArgs::scalar_arguments` (literals only, `expr_schema.rs:583-589`); `ScalarUDF::call` (`udf.rs:172`); `Signature::{user_defined, with_parameter_names}`; `ExtensionTypeRegistration` (`registry.rs:351-420`) |
| datafusion | 54.0.0 | `SessionStateBuilder::with_type_planner` (`session_state.rs:1295`); `FunctionRegistry::{register_expr_planner, register_function_rewrite}` through `SessionContext::state_ref().write()` (`context/mod.rs:2034`) |
| datafusion-sql | 54.0.0 | `convert_data_type_to_field` asks the type planner first (`planner.rs:648-656`), used by casts, `TRY_CAST`, typed strings and `CREATE TABLE`; extension planners run before `parse_sql_binary_op` (`expr/mod.rs:123-150`) |
| datafusion-common | 54.0.0 | `plan_err!`, `exec_err!`, `not_impl_err!`, `internal_err!`, `*_datafusion_err!`; `utils::take_function_args` (`utils/mod.rs:1053`); `config::Dialect::PostgreSQL` (`config.rs:334,374`) |
| datafusion-macros, datafusion-doc | 54.0.0 | `#[user_doc]`; generated code names `datafusion_doc::*` (`user_doc.rs:255-273`) |
| sqlparser | 0.62.0 | `ast::DataType::Custom(ObjectName, Vec<String>)` (`data_type.rs:439`); `BinaryOperator::{PGOverlap, At, TildeEq, LtDashGt, AndLt, AndGt, LtLtPipe, PipeGtGt, AndLtPipe, PipeAndGt, PGRegexMatch, PGBitwiseShiftLeft, PGBitwiseShiftRight, Custom}`; `PostgreSqlDialect::is_custom_operator_part` (`dialect/postgresql.rs:95-113`) |
| geoarrow-schema | 0.8.0 | `Metadata::{new, crs, edges}`, `Metadata: TryFrom<&Field>` (`metadata.rs`); `Crs::{from_authority_code, from_srid, crs_type, crs_value}` (`crs.rs`; `from_authority_code` asserts a `:`); `Edges::Spherical`; `WkbType`, `BoxType`; `GeoArrowType::{from_arrow_field, with_metadata}` (untagged `Binary`/`Utf8` fall back to WKB/WKT, `datatype.rs:356-396`) |

### 5.2 Upstream gaps

| # | Project | Gap | Impact |
|---|---|---|---|
| 1 | DataFusion | `Cast` ignores target field metadata (fixed in 55) | VALUES lose geometry type/CRS; operator detection needs the storage-type test |
| 2 | geoarrow-rs | No release on arrow 59 | Blocks DataFusion 55, so blocks 1 |
| 3 | DataFusion | INSERT uses type-only casts to the table schema | `INSERT ... VALUES ('POINT(1 2)')` stores text bytes |
| 4 | DataFusion | Single type planner per session; not settable on a `SessionContext` | `register` can't install the types |
| 5 | DataFusion | `OneOf` mismatch errors read "Internal error ... likely caused by a bug in DataFusion" (observed) | Ugly errors for overload misses (one reason to prefer `user_defined`) |
| 6 | DataFusion | `user_defined` coercion failures are wrapped as execution errors (`type_coercion/functions.rs:536-541`) | Plan errors print as "Execution error: ... user-defined coercion failed with: Error during planning: ..." |
| 7 | datafusion-ffi | No planner/rewrite FFI | Python gets no types, casts or operators |
| 8 | sqlparser | Typed literals of custom types; composite field access | `geometry 'POINT(1 2)'`; `(ST_Dump(g)).geom` |
| 9 | geoarrow/wkt/wkb | Curves and surfaces | 57 records |

## 6. Phasing

Numbers are failing records (of 527) that contain the feature, from the parsed `.slt` files; a
record may need other missing functions too.

| Feature | Records | Files |
|---|---|---|
| SRID model: `'SRID=..'` literals, ST_GeomFromEWKT, ST_AsEWKT, ST_SetSRID, ST_SRID, ST_Transform | 154 | 90 |
| of which `'SRID=..'` literals or ST_GeomFromEWKT (G4) | 100 | 61 |
| of which ST_AsEWKT (G4) | 74 | 47 |
| ST_GeomFromText/ST_GeomFromWKB with `srid` (G4, "coercion from Utf8, Int64") | 13 | |
| Spatial operators | 30 | 28 |
| `::geometry` literal (today via the shim) | 129 | 92 |
| geography | 10 | 8 |
| ST_SetSRID / ST_SRID | 11 | 9 |
| non-literal `::geometry`, `::text` | 6 | 7 |

**Batch 1: foundations (blocks every group).** `src/util/` (R1) with `coerce_args`, argument
readers, field helpers and the SRID helpers; errors (R3, including `error.rs` and the
`dep:geos`/`sql` features); `#[user_doc]` migration and the documentation test (R4); the type
model rule (R5) written into the style guide; R8. Migrate the exemplars: Area, Centroid, the
point constructors (SRID via `scalar_srid`), Box2D/Box3D, and the four `Signature::any` users.
Add ST_SRID and ST_SetSRID. Direct gain: st_srid 1, st_setsrid 1 (the other needs ST_Transform).
Indirect: G4's EWKT/EWKB input and output (up to 154 records) and every `srid` argument (G1, G4)
can start.

**Batch 2: operators.** Harness dialect switch to PostgreSQL; `GeoExprPlanner` (it needs the
`sql` feature but no types); the predicate functions in `udf/native/operators/`; `<->`, `<#>`,
`<<->>` with hand-written tests; `|=|` once G1 has ST_DistanceCPA. Gain: 24 records directly
(geometry_overlaps 1, geometry_overlaps_nd 2, the ten `box2df`/`gidx` files, st_geometry_* 11);
`contains_box2df_geometry` and `contains_geometry_box2df` also need ST_Buffer.

**Batch 3: types and casts.** `GeoTypePlanner`, `geometry(...)`/`geography(...)` (on G4's
parsers), `GeoCastRewrite`, `CREATE TABLE`. The harness builds its session with the type planner
and gets a switch to run without the shim. Gain: the non-literal casts (st_clusterintersectingwin 1,
st_clusterwithinwin 1, st_ashexewkb 1, and st_point 2-3 once geography exists). Then compare parity with
and without the shim. On DataFusion 54 records that returned a geometry straight from `VALUES`
would regress without the shim (gap 1); on 55 (D17) batches 3 and 5 go together. R9 here too.

**Batch 4: geography.** Geography metadata, `geography(...)`, `common_metadata`'s geometry vs
geography check, `is_geography`; G2/G3 add the overloads. Gain: up to 10 records with G2.

**Batch 5: after DataFusion 55.** Remove the shim, render from the physical plan schema, drop the
storage-type test in the operator planner, consider the INSERT analyzer rule.

## 7. Per-item notes

| Item | Approach | PostGIS gotchas | Difficulty | Doc tests |
|---|---|---|---|---|
| `geometry` type | `GeoTypePlanner` → WKB field | Typmod `geometry(Point, 4326)`: type names case-insensitive, `Z`/`M`/`ZM` suffixes, SRID optional; a geometry with SRID 0 cast to a typmod column takes the column's SRID | M | — (hand-written) |
| `geography` type | WKB + `edges: spherical` + CRS | Default SRID 4326 (`ST_SRID('POINT(1 2)'::geography)` = 4326). Only lon/lat CRSs are allowed. `geography → geometry` is explicit only, `geometry → geography` implicit. Functions without a geography overload error (`ST_X(geography)` doesn't exist) | M | 10 records via G2 |
| `box2d`, `box3d` types | `BoxType` XY/XYZ | `'BOX(1 2,5 6)'::box2d` text input; `box2d::text` prints `BOX(1 2,5 6)` | S | box2d 1/2 (curve), box3d 0/2 (G4 EWKT) |
| `geometry(...)` cast function | Parses with G4; returns the target field | Text input accepts WKT, EWKT and HEXEWKB; SRID mismatch with typmod errors; typmod type mismatch errors in PostGIS (not enforced here) | M | via every `::geometry` |
| `::text`, `::bytea` | Rewrite to ST_AsHEXEWKB / ST_AsEWKB | `text(geometry)` is uppercase hex EWKB with SRID flag, e.g. `0101000020E6100000...` | S | st_ashexewkb 1 |
| ST_SRID | Int32 per row from `crs_to_srid` | NULL in, NULL out; geography defaults to 4326; a CRS without an SRID gives 0 (documented) | S | st_srid 0/1 |
| ST_SetSRID | Replace the CRS, keep storage and edges | `srid` must be constant here; SRID < 0 → 0 and > 999999 is folded (PostGIS emits a NOTICE); NULL SRID → NULL | S | st_setsrid 0/2 (+ 8 other files) |
| `&&` | Bounding boxes intersect (2D) | Doesn't check SRIDs (`'SRID=3857;..' && 'SRID=4326;..'` is true); boxes as operands; EMPTY → false | S | 4 files |
| `&&&` | n-D box intersect (Z and M when both have them) | 2D operands compare XY only | S | geometry_overlaps_nd 2, gidx 3 |
| `~`, `@` | Box containment | Box operands (struct type); mixed with geometry | S | 3 + 3 + 2 |
| `~=` | Box equality | Not geometric equality: `LINESTRING(0 0,1 1) ~= LINESTRING(0 1,1 0)` is true | S | 1 |
| `<<`, `>>`, `&<`, `&>`, `<<\|`, `\|>>`, `&<\|`, `\|&>` | Box position | "Strictly left" uses `xmax < xmin`; overleft is `xmax <= xmax` | S | 8 |
| `<->` | True 2D distance; geography: sphere | KNN `ORDER BY a <-> b LIMIT k` is a sort, no index | S | — |
| `<#>` | Box-to-box distance | | S | — |
| `<<->>` | n-D distance of box centroids | Uses M when present | S | — |
| `\|=\|` | ST_DistanceCPA (G1) | Requires M-valued linestrings | — | — |
| `=` | DataFusion bytewise equality | PostGIS compares type, coordinates and SRID (`'SRID=4326;POINT(1 2)' = 'POINT(1 2)'` is false). With column-level CRS and WKB storage, bytes differ only in byte order; overriding `=` would stop DataFusion using hash joins | — | st_geometry_eq 4/4 |
| SRID model | `util::srid`, `scalar_srid`, `common_metadata` | Mixed SRIDs error per row in PostGIS; per column here. ESRI codes are not EPSG codes | S | 154 records via G4/G3 |
| Shared scaffolding | Section 4 | — | M | all |

## 8. Testing

- **Pure helpers** (`util::srid`, `coerce_args`, `GeoTypePlanner`) get unit tests in their files.
  Type planner tests parse a type with sqlparser
  (`Parser::new(&PostgreSqlDialect {}).try_with_sql("geometry(PointZ, 4326)")?.parse_data_type()`)
  and assert the field's data type, extension name and `Metadata`.
- **Planner, rewrite and operator tests** are SQL in a `SessionContext` built with
  `GeoTypePlanner` and the PostgreSQL dialect. Assert on the *physical* output schema
  (`df.create_physical_plan().await?.schema()`), not `df.schema()`: on DataFusion 54 the
  logical schema of a cast lacks the extension until the analyzer has run (verified).
- **Behaviour** goes in hand-written `.slt` files recorded from PostGIS: `geometry_type.slt` (casts
  from text/WKT/EWKT/HEXEWKB/bytea, typmods, NULL, `::text`, `::bytea`, SRID mismatch errors,
  `CREATE TABLE` + `INSERT ... SELECT`), `geography_type.slt`, `box2d_type.slt`, `operators.slt`
  (every operator with geometry/box operands, EMPTY, NULL, mixed SRIDs), `st_srid.slt`,
  `st_setsrid.slt`. Error text isn't compared (`query error` only).
- **Documentation test** in `lib.rs` (R4).

Harness changes, in order:

1. Batch 2: set `datafusion.sql_parser.dialect = 'PostgreSQL'` in `GeoDataFusion::new`
   (`datafusion_engine.rs:28-32`). No record parses worse; 16 more parse.
2. Batch 3: build the session with `SessionStateBuilder` and `GeoTypePlanner`. Add a switch
   (environment variable) that disables `rewrite_geometry_literals`, and compare parity with and
   without it.
3. With G4 R1: drop the `geoarrow.wkt` special case (`datafusion_engine.rs:94-96`), since text
   outputs are plain `Utf8`.
4. Batch 5: remove the shim and render with the physical plan's schema instead of
   `batch.schema()`, because `MemTable` scans drop metadata on DataFusion 54 (verified), so
   `CREATE TABLE` tests would otherwise render geometries as bytes. Update the README's shim
   section and the skill's note on `::geometry` literals.

`srid_from_crs` (`datafusion_engine.rs:138-148`) stays a separate implementation: the renderer
shouldn't share code with what it checks.

## 9. Style guide amendments

Proposed edits to `STYLE_GUIDE.md`, in its sections:

**Layout.** Replace "Signature helpers shared crate-wide live in `data_types.rs`" with: crate-wide
helpers live in `src/util/` (`signature`, `args`, `field`, `srid`, plus G1's kernel modules); the
SQL types, casts and operators live in `src/sql/` behind the `sql` feature. Category helpers stay
in a category `util` module. Add chapters `types`, `srs`, `operators` to the category list. In the
GEOS bullet: the error variant and any shared GEOS code are gated on `geos-3_11`, the lowest GEOS
feature, and features use `dep:` so no implicit `geos` feature exists.

**Anatomy of a UDF.** Replace the reference example with the template in 4.5. Rules:

- Documentation is a `#[user_doc]` attribute above the derives; `documentation()` returns
  `self.doc()`.
- Trait method order: `name`, `aliases`, `signature`, `return_type`, `return_field_from_args`,
  `coerce_types`, `invoke_with_args`, `simplify`, `documentation`.
- Signatures: one geometry argument → `single_geometry()`. Anything else →
  `Signature::user_defined` with a file-level `static ARGUMENTS: &[&[Arg]]` (commented with the
  PostGIS synopsis) and `coerce_types` delegating to `coerce_args`. Add
  `with_parameter_names` with PostGIS's parameter names. Never `Signature::any`, and no new
  `Exact` lists over `any_geometry_type()`.
- Return types: a fixed type from `return_type`; a geometry from `return_field_from_args` through
  `geometry_return_field`, with `return_type` returning
  `internal_err!("return_field_from_args should be called instead")`. Return fields are named
  `self.name()`.

**Inputs.** Decode geometry arguments with `geometry_array(&args, i)`, not `from_arrow_array`.
Read other arguments with the `util::args` readers, per row. Only arguments that set the output
type or CRS are constant-only, read when planning with `scalar_srid`/`scalar_text`. Replace the
"return `NotImplemented` for arrays" rule.

**A new "Types and SRIDs" section** (after Outputs):

- A `geoarrow.*` extension means geometry, geography or box. Text and binary results (ST_AsText,
  ST_AsBinary, ...) are plain `Utf8`/`Binary`.
- Geography is `edges: "spherical"`. Test it with `is_geography`; branch once at the top of the
  `_impl` function.
- The SRID is the column's CRS. Convert only with `util::srid` (`srid_to_crs`, `crs_to_srid`);
  never format `"EPSG:{n}"`. Functions with several geometry arguments check SRIDs with
  `common_metadata`: when planning if they return a geometry, otherwise at the start of the
  `_impl`.

**Errors.** Replace the variant bullets with:

- Use DataFusion's macros: `plan_err!` (arguments invalid when planning, including non-constant
  SRIDs), `exec_err!` (invalid data, mixed SRIDs), `not_impl_err!` (supported in PostGIS, not yet
  here), `internal_err!` (bugs only). In `GeoDataFusionResult` functions:
  `return Err(exec_datafusion_err!(..).into())`.
- Helpers that only raise DataFusion errors return `datafusion::error::Result`;
  `GeoDataFusionResult` is for code that calls GeoArrow, `geo`, GEOS or other crates.
- Messages start with `self.name()`: `"{}: Operation on mixed SRID geometries", self.name()`.
  Follow PostGIS's wording where there is one.
- No `unreachable!()` for input types; the signature allows more types than you think (`Null`,
  views).

**Documentation.** Replace the section:

- `#[user_doc]`, with `doc_section(label = ...)` set to the PostGIS reference chapter: "Data Types",
  "Geometry Constructors", "Geometry Accessors", "Geometry Editors", "Geometry Validation",
  "Spatial Reference System Functions", "Geometry Input", "Geometry Output", "Operators",
  "Spatial Relationships", "Measurement Functions", "Overlay Functions", "Geometry Processing",
  "Coverages", "Affine Transformations", "Clustering Functions", "Bounding Box Functions",
  "Linear Referencing", "Trajectory Functions". The documentation test enforces the list.
- `description`: the PostGIS one-line summary, then differences ("Unlike PostGIS, ...").
- `syntax_example`: `ST_Name(arg1, arg2)` with PostGIS parameter names; operators use
  `alternative_syntax = "geomA && geomB"` in addition.
- `argument(name = ..., description = ...)`: the description is the SQL type (`geometry`,
  `geography`, `box2d`, `float8`, `integer`, `boolean`, `text`), optionally followed by a short
  phrase (`"float8, in the units of the SRS"`).
- `related_udf(name = ...)` for close siblings.

**Tests.** Note that operator tests need the PostgreSQL dialect (the harness sets it), and update
the `::geometry` sentence when the shim is removed.

**Python bindings.** Add: the SQL types, casts and operators aren't available through Python
(no FFI for planners); don't add Python classes for operator functions beyond the UDFs.

## 10. Open questions for the maintainer

1. **Output encoding.** Keep native GeoArrow outputs with `coord_type` (recommended, and G4's
   recommendation for input functions), accepting that `UNION`/`CASE`/`COALESCE` mixing WKB and
   native geometries need a `::geometry` cast? Or make WKB the default output so every geometry
   has one Arrow type, as in PostGIS and SedonaDB?
2. **`geometry` storage:** `Binary` (recommended, widest support) or `BinaryView`?
3. **SRID 4326:** keep writing `EPSG:4326` (today's convention, PostGIS treats it as lon/lat), or
   `OGC:CRS84`, which GeoParquet uses for lon/lat?
4. **Per-row SRIDs in column input** (EWKT/EWKB columns): an execution error unless they match
   the column (recommended, G4's proposal too), or drop them silently?
5. **API:** `sql` feature on by default? Expose `GeoTypePlanner` as a public struct users pass to
   `SessionStateBuilder` (recommended), or a helper that builds the whole session? Keep
   `register(&SessionContext)` returning `()`?
6. **Dialect:** document `SET datafusion.sql_parser.dialect = 'PostgreSQL'` for operators
   (recommended), or have `register` change the session's dialect?
7. **DataFusion 55:** upgrade as soon as geoarrow has an arrow-59 release, and gate shim removal
   on it?
8. **`#[user_doc]`:** one migration PR in batch 1 (recommended), or per group as files are
   touched? It adds `datafusion-doc` and `datafusion-macros` as dependencies.
9. **Mixed-SRID timing:** at execution for fixed-return functions (like PostGIS) and at planning
   for geometry-returning ones (needed for the output CRS)? The alternative is planning
   everywhere, which means `return_field_from_args` on every UDF.
10. **Typmod subtypes:** leave `geometry(Point, 4326)` unenforced (recommended for now), or map it
    to a native `geoarrow.point` column, which enforces the type and changes storage?
11. **ESRI table:** OK to vendor the 2315 ESRI SRIDs from `spatial_ref_sys`, generated by a dev
    script?
12. **INSERT casts:** worth a custom analyzer rule for `INSERT INTO t VALUES ('POINT(1 2)')` into
    a geometry column, or wait for upstream?
