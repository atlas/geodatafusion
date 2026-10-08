# Style Guide

This guide describes how code, comments, tests and documentation in `geodatafusion` are
written. It applies to humans and AI agents alike. The goal is that every function reads as if
the same person wrote it, and that the crate looks familiar to anyone who knows
[DataFusion](https://github.com/apache/datafusion) and [GeoArrow](https://github.com/geoarrow/geoarrow-rs).

When this guide and existing code disagree, follow this guide. If you're touching that code
anyway, bring it in line. When this guide is silent, copy the closest function in the same group
(see [`plans/README.md`](plans/README.md)), then the conventions of `datafusion-functions`.

> **Status.** Some conventions depend on shared helpers in `src/util/` that are being introduced
> by the foundations phase of the [parity plan](plans/README.md#phasing) (G6 batch 1). Existing
> code is migrated in that phase. If a helper named here doesn't exist yet, add it to
> `src/util/` as specified in [`plans/g6-types-infrastructure.md`](plans/g6-types-infrastructure.md)
> before using it. Don't write a local substitute.

See also [DEVELOP.md](DEVELOP.md) for the development workflow and
[`tests/sqllogictests/README.md`](rust/geodatafusion/tests/sqllogictests/README.md) for the
PostGIS parity tests.

## Principles

1. **PostGIS is the specification.** Function names, argument order, argument names, defaults,
   NULL handling, return types and edge-case behaviour follow the
   [PostGIS reference](https://postgis.net/docs/reference.html). Where we differ, the difference
   is deliberate and documented (see [Documentation](#documentation)).
2. **DataFusion is the host.** UDFs use DataFusion's traits and idioms (`ScalarUDFImpl`,
   `Signature`, `#[user_doc]`, `ColumnarValue`, its error macros), and don't invent parallel
   abstractions.
3. **GeoArrow is the data model.** Geometries are GeoArrow arrays. Inputs accept every GeoArrow
   encoding. Geometry outputs are WKB (`geoarrow.wkb`) with the CRS of the inputs, like
   PostGIS's single `geometry` type.
4. **Consistency beats local cleverness.** A new function is mechanically identical to its
   siblings: same file shape, same helpers, same loop, same error handling, same tests.

## Layout

```
rust/geodatafusion/src/
├── lib.rs        register(): calls every category's register()
├── error.rs      GeoDataFusionError
├── util/         crate-wide helpers: kernel, signature, args, field, srid, ordinates, owned, ...
├── sql/          SQL types, casts and operators (behind the `sql` feature)
└── udf/
    ├── native/   implemented from scratch on geo-traits / GeoArrow arrays (G1, G4)
    ├── geo/      wrappers around the `geo` crate (G2)
    ├── geos/     wrappers around the GEOS C library, behind `geos-*` features (G3)
    └── proj/     wrappers around PROJ, behind the `proj` feature (G3)
```

- **Providers.** One implementation per SQL function, in the provider whose result matches
  PostGIS: `geo` when it matches, otherwise GEOS when PostGIS uses GEOS, otherwise native.
  "Matches" means ≥ 99.9% agreement on the backend agreement corpus
  ([experiment E2](plans/experiments/e2-backend-agreement.md)), with every disagreement cheaply
  fixable. Run that test before putting a new function on `geo`. Anything that reads or writes Z
  or M is native, because `geo` is 2D. Never switch implementations by feature.
- **Categories.** Below the provider, functions are grouped by category, named after the PostGIS
  reference chapter in snake_case: `accessors`, `constructors`, `editors`, `bounding_box`, `io`,
  `measurement`, `processing`, `relationships`, `validation`, `affine_transformations`,
  `linear_referencing`, `trajectory`, `overlay`, `clustering`, `srs`, `types`, `operators`.
- **Files.** One file per function, named after the function without `ST_` in snake_case
  (`ST_NumInteriorRings` → `num_interior_rings.rs`). Closely related variants share a file and
  the file starts with a `//!` doc listing them: `ST_Point`/`ST_PointZ`/`ST_PointM`/`ST_PointZM`;
  `ST_AsText`/`ST_AsEWKT`; a type-checked family like `ST_GeomFromText`/`ST_PointFromText`/...; a
  scalar and its aggregate form (`ST_Collect`).
- **`mod.rs`.** A category's `mod.rs` contains only private `mod` declarations, `pub use`
  re-exports and a `register` function, which registers every UDF in the category. Every
  category's `register` is called from `geodatafusion::register` in `lib.rs`:

  ```rust
  mod area;
  mod distance;

  pub use area::Area;
  pub use distance::Distance;

  pub fn register(session_context: &datafusion::prelude::SessionContext) {
      session_context.register_udf(Area.into());
      session_context.register_udf(Distance.into());
  }
  ```

- **Helpers** live at the narrowest level that covers all their users: crate-wide in `src/util/`,
  for one provider in `udf/<provider>/util/` (e.g. the GEOS bridge), for one category in
  `udf/<provider>/<category>/util/` (e.g. the number formatter in `native/io/util/`). Don't add a
  second version of an existing helper; extend the existing one.
- **GEOS features.** `geos-3_11` is the floor: it gates the whole `geos/` provider, its `util`
  module and the GEOS error variant. A function that needs a newer GEOS also gets
  `#[cfg(feature = "geos-3_x")]` on its `mod`, `pub use` and `register` line. Higher features
  imply lower ones. Optional dependencies are declared with `dep:`, so there are no implicit,
  unversioned features.

## Naming

| Thing | Convention | Example |
|---|---|---|
| UDF struct | PostGIS name without `ST_`, keeping PostGIS's capitalisation inside the name | `NumInteriorRings`, `AsEWKT`, `GeomFromWKB`, `Box2dFromGeoHash` |
| Aggregate form of a scalar | `<Struct>Agg` | `CollectAgg` |
| SQL name (`fn name`) | full PostGIS name, lowercase | `"st_numinteriorrings"` |
| SQL name of an aggregate whose scalar exists | The PostGIS name, once DataFusion falls back from a scalar to a same-named aggregate; `st_<name>_agg` only if that stalls | `"st_collect"` |
| Aliases | PostGIS aliases, lowercase, in `aliases()` | `"st_length2d"` |
| Implementation fn | `<file_name>_impl` | `num_interior_rings_impl` |
| Kernel | `<Struct>Kernel` | `NumInteriorRingsKernel` |
| Python class | `Py<Struct>`, exported as `"<Struct>"` | `PyArea` / `"Area"` |

If the PostGIS name without `ST_` collides with another PostGIS function (`GeometryType` vs
`ST_GeometryType`), keep the `ST_` prefix on the struct and allow `non_camel_case_types` on it.

## Anatomy of a scalar UDF

Every scalar UDF has this shape:

```rust
use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::affine_transformations::util::Affine3D;
use crate::util::args::float_arg;
use crate::util::field::{geometry_array, geometry_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_coords;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Translate(geometry g1, float8 deltax, float8 deltay).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "deltax", "deltay"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Translates a geometry by given offsets.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Returns a new geometry whose coordinates are translated by deltax and deltay. Z and M values are unchanged.",
    syntax_example = "ST_Translate(g1, deltax, deltay)",
    argument(name = "g1", description = "geometry"),
    argument(name = "deltax", description = "float8"),
    argument(name = "deltay", description = "float8"),
    related_udf(name = "st_affine")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Translate;

impl Translate {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Translate {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Translate {
    fn name(&self) -> &str {
        "st_translate"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        geometry_return_field(self.name(), &args, &[0])
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(translate_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn translate_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = TranslateKernel {
        deltax: float_arg(&args, 1)?,
        deltay: float_arg(&args, 2)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct TranslateKernel {
    deltax: Float64Array,
    deltay: Float64Array,
}

impl GeometryKernel for TranslateKernel {
    type Output = wkt::Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<wkt::Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        if self.deltax.is_null(row) || self.deltay.is_null(row) {
            return Ok(None);
        }
        // PostGIS implements ST_Translate as ST_Affine, so use the same matrix to get the same
        // floating-point results.
        let affine = Affine3D::translate(self.deltax.value(row), self.deltay.value(row), 0.0);
        Ok(Some(map_coords(geom, &|c| affine.apply(c))?))
    }
}
```

Rules:

- **Order of items:** `use`s, then signature statics, then the struct with its `#[user_doc]`,
  `new`, `Default`, the trait impl, then the free functions doing the work and the kernel, then
  tests.
- **Derives:** exactly `#[derive(Debug, Eq, PartialEq, Hash)]`, after `#[user_doc]`.
- **Struct doc comment:** one `///` line, normally the PostGIS one-line summary.
- **Constructors:** always `new()` plus `Default`, with `Default` delegating to `new`. UDF
  structs are unit structs; geometry outputs are always WKB, so there's no `coord_type` field.
- **Trait method order:** `name`, `aliases`, `signature`, `return_type`,
  `return_field_from_args`, `coerce_types`, `invoke_with_args`, `simplify`, `documentation`.
  Don't implement `as_any` or other defaulted methods without a reason.
- **Signatures:** one geometry argument and nothing else → `single_geometry()`. Anything else →
  `Signature::user_defined` with a file-level `static ARGUMENTS: &[&[Arg]]` (commented with the
  PostGIS synopsis, one entry per PostGIS overload, shorter entries for `DEFAULT` parameters) and
  `coerce_types` delegating to `coerce_args`. Add `with_parameter_names` with PostGIS's parameter
  names. Never use `Signature::any`, and don't build `Exact` lists over geometry types. Volatility
  is `Immutable` unless the function is really random.
- **Return types:** a fixed type comes from `return_type`. A geometry comes from
  `return_field_from_args` via `geometry_return_field` (WKB, with the inputs' CRS), and
  `return_type` then returns `internal_err!("return_field_from_args should be called instead")`.
- **Thin trait methods:** `invoke_with_args` and `return_field_from_args` only delegate to free
  functions, converting `GeoDataFusionResult` with `Ok(..?)`. All logic lives in those free
  functions.

## Other UDF kinds

- **Aggregates** implement `AggregateUDFImpl` with methods in the order `name`, `aliases`,
  `signature`, `return_type`, `return_field`, `accumulator`, `state_fields`,
  `groups_accumulator_supported`, `create_groups_accumulator`, `order_sensitivity`,
  `documentation`. They always implement `state_fields` when the state isn't the return value.
  Aggregates that gather geometries and compute once use the shared collect accumulator
  (`util::collect`), with the finalize step shared with the scalar form. Constant parameters
  (grid size, distance) are read once with `util::literal::literal_arg`.
- **Window functions** implement `WindowUDFImpl`: `name`, `aliases`, `signature`,
  `partition_evaluator`, `field`, `documentation`.
- **Set-returning functions** (ST_Dump and friends) are scalar UDFs returning
  `List<Struct<...>>` with PostGIS's column names, registered with `register_set_returning` so
  that the `FROM st_dump(...)` form works too.

The templates are in [`plans/g5-aggregate-window-set.md`](plans/g5-aggregate-window-set.md).

## Implementation

### Rows

Rows are read with typed access. Never convert a geometry array to WKB just to read it:
[experiment E1](plans/experiments/e1-performance.md) measured that at 1.4–460× slower on native
inputs.

- **The first geometry argument** goes through a `GeometryKernel` and the drivers in
  `util::kernel`: `map_geometry` for an Arrow result (`BooleanArray`, `Float64Array`, ...) and
  `map_geometry_to_wkb` for a geometry result. The drivers dispatch on the array type, append NULL
  for NULL rows and propagate errors. `eval` gets a non-NULL geometry and its row index, and
  returns `Ok(None)` for a NULL result.
- **Other arguments** are held by the kernel and read by row index: non-geometry arguments as
  arrays from the `util::args` readers, further geometry arguments as columns materialised once
  by typed iteration (`udf::geo::util::GeoColumn`, `udf::geos::util::GeosColumn`, or
  `util::owned::OwnedColumn` for native code). A constant geometry is converted, and prepared
  where the backend supports it, once.
- **Backends convert inside `eval`:** `geo` code calls `udf::geo::util::geometry_to_geo`, which
  returns `GeoValue::Empty(kind)` or `GeoValue::Geometry(geo::Geometry)`, and matches both arms
  explicitly so EMPTY behaviour is visible. GEOS code calls `udf::geos::util::to_geos`, after
  applying PostGIS's own EMPTY and point-in-polygon rules.
- Don't call `downcast_geoarrow_array!` outside the drivers. A per-function fast path over a
  specific array type needs a benchmark that justifies it.
- Keep the shortcuts PostGIS-equivalent kernels can take for cheap inputs (points have no area
  or length); E1 found them worth 35–120×.
- Read Z and M with `util::ordinates::{z, m}`, never `CoordTrait::nth(2)` (which is M for XYM).
  Owned geometries that must keep Z/M are `wkt::Wkt<f64>`, built with `util::owned`.
- Import `geo` algorithm traits anonymously (`use geo::Area as _;`), since several share their
  name with UDF structs.

### Arguments

- Non-geometry arguments are read per row with the `util::args` readers (`float_arg`,
  `optional_float_arg`, `int_arg`, `bool_arg`, `text_arg`, ...), which broadcast constants.
  PostGIS `integer` parameters are `Int32`.
- Only arguments that set the output type or CRS (an SRID, a target projection) are
  constant-only, read in `return_field_from_args` with `scalar_srid`/`scalar_text`. A
  non-constant value there is a plan error.
- Missing optional arguments take the PostGIS default.

### NULLs, EMPTY and edge cases

- NULL in any argument gives NULL, because PostGIS functions are `STRICT`. The exception is a
  PostGIS function whose `pg_proc.proisstrict` is false; check it.
- EMPTY geometries, collections, Z/M dimensions and SRIDs behave as in PostGIS. Where the
  underlying library differs, correct it and say so in a comment:

  ```rust
  // PostGIS returns the original geometry for empty input,
  // whereas GEOS would collapse it to an empty GeometryCollection.
  ```

- Where PostGIS raises an error (ST_X on a polygon), raise an error. Don't return NULL instead.
- When PostGIS defines a function in terms of another (ST_Rotate as ST_Affine), compute it the
  same way, so floating-point results match.

### Outputs

- Fixed return types map PostGIS SQL types one to one: `integer` → `Int32`, `smallint` →
  `Int16`, `bigint` → `Int64`, `float8` → `Float64`, `boolean` → `Boolean`, `text` → `Utf8`,
  `bytea` → `Binary`. No unsigned integers, no `Utf8View`, and no GeoArrow extension on text or
  binary output, even when it holds WKT or WKB.
- Geometry outputs are WKB (`geoarrow.wkb` on `Binary`), whatever the input encoding, built by
  `map_geometry_to_wkb` from `args.return_field` so the declared field and the array can't
  disagree. WKB outputs combine in `UNION`, `CASE` and `COALESCE` where mixed native types don't,
  avoid geoarrow-rs's GeometryCollection bugs, hold nested collections (which the native union
  can't), and weren't slower in any measured pipeline (E1, [E4](plans/experiments/e4-type-model.md),
  [E7](plans/experiments/e7-union-outputs.md)). Boxes (`box2d`/`box3d`) stay `geoarrow.box`.
- Return fields are named `self.name()` and are nullable unless the function can never return
  NULL.
- Coordinates in text output are written with `write_number` (`native/io/util/number.rs`), which
  reproduces PostGIS's formatting. Never format an `f64` with `{}` in user-visible text.
- Results are always arrays (`ColumnarValue::Array`). DataFusion turns them back into scalars.

### Types and SRIDs

- A field with a `geoarrow.*` extension is a geometry, geography or box. Plain `Utf8` is `text`
  and plain `Binary` is `bytea`.
- A geography is a geometry field with GeoArrow `edges: "spherical"`. Test it with
  `util::field::is_geography` and branch once at the top of the `_impl` function.
- The SRID is the column's CRS. Convert between them only with `util::srid` (`srid_to_crs`,
  `crs_to_srid`); never build a CRS yourself. `crs_to_srid` reads every form producers write
  (PROJJSON, `OGC:CRS84`, authority codes). `srid_to_crs` writes the authority code PostGIS
  uses for the SRID (`EPSG:4326`, `ESRI:102003`), which round-trips through GeoPandas, DuckDB
  and GDAL in Arrow hand-offs; GeoParquet writing expands it to full PROJJSON (E4).
- Functions with several geometry arguments check SRIDs with `common_metadata` at the start of
  the `_impl` function. It skips arguments that are NULL in every row, because PostGIS functions
  are STRICT.

### Errors

- No `unwrap()`, `expect()`, `panic!`, `unreachable!()` or fallible indexing on anything that
  depends on user input. Propagate with `?`. `expect` is fine for invariants that planning
  guarantees, with a message saying why.
- Use DataFusion's error macros: `plan_err!` (invalid arguments known when planning), `exec_err!`
  (invalid data, mixed SRIDs), `not_impl_err!` (supported by PostGIS but not yet here),
  `internal_err!` (bugs only). In `GeoDataFusionResult` functions, use
  `return Err(exec_datafusion_err!(...).into())`.
- Helpers that only raise DataFusion errors return `datafusion::error::Result`.
  `GeoDataFusionResult` is for code that calls GeoArrow, `geo`, GEOS or other crates; their
  errors are variants of `GeoDataFusionError`.
- Messages start with the function name: `exec_err!("{}: Operation on mixed SRID geometries",
  self.name())`. Follow PostGIS's wording where there is one.

### Comments

- Explain *why*, not *what*. The best comments cite PostGIS, JTS or GEOS behaviour and explain
  the choice.
- Full sentences, ending in a period. `TODO:` comments say what's missing.
- Use `//!` module docs only when a file holds several UDFs or non-obvious shared machinery.

### Macros

`macro_rules!` may generate a family of five or more near-identical UDFs in one file (the
spatial predicates), expanding to exactly the anatomy above. Otherwise, write each UDF out.

### Licensing

The crate is MIT/Apache-2.0. Implement from the PostGIS documentation and observed behaviour.
Don't copy or translate PostGIS/liblwgeom (GPL-2.0) or GEOS (LGPL-2.1) source. Algorithms from
papers, JTS (EDL) and `geo` (MIT/Apache) may be used, with a comment crediting the source.

## Documentation

- Every UDF has a `#[user_doc]` attribute, and `documentation()` returns `self.doc()`.
- `doc_section(label = ...)` is the PostGIS reference chapter: "Data Types", "Geometry
  Constructors", "Geometry Accessors", "Geometry Editors", "Geometry Validation", "Spatial
  Reference System Functions", "Geometry Input", "Geometry Output", "Operators", "Spatial
  Relationships", "Measurement Functions", "Overlay Functions", "Geometry Processing",
  "Coverages", "Affine Transformations", "Clustering Functions", "Bounding Box Functions",
  "Linear Referencing", "Trajectory Functions". A unit test in `lib.rs` enforces the list.
- `description`: the PostGIS one-line summary, then any behaviour a user needs to know, especially
  *differences from PostGIS*, in standard wording: "Unlike PostGIS, ...", "Unlike PostGIS, the
  result is always 2D.", "This function drops the M coordinate.", "Requires GEOS 3.12 or later."
- `syntax_example`: `ST_Name(arg1, arg2)` with PostGIS parameter names. Operators also give
  `alternative_syntax = "geomA && geomB"`.
- `argument(name = ..., description = ...)`: PostGIS parameter names; the description is the SQL
  type (`geometry`, `geography`, `box2d`, `float8`, `integer`, `boolean`, `text`), optionally
  followed by a short phrase (`"float8, in the units of the SRS"`).
- `related_udf(name = "st_other")` for close siblings, lowercase.
- Update the function table in `README.md` (✅ in *Implemented*) in the same change.

## Python bindings

Every UDF is exposed to Python in the same change:

1. `impl_udf!(Struct, PyStruct, "Struct")` (`impl_udaf!`/`impl_udwf!` for aggregates and window
   functions) in
   `python/src/udf/<provider>/<category>.rs`.
2. `m.add_class::<<category>::PyStruct>()?;` in the provider's `#[pymodule]`, under the
   category comment.
3. A class stub in `python/python/geodatafusion/<provider>/_<category>.pyi`, with the dunder
   matching the UDF kind.

The wheels bundle GEOS, so GEOS functions are bound too. Exceptions: PROJ functions until the
wheels ship PROJ and `proj.db`, and SQL types, casts and operators (DataFusion has no FFI for
planners).

## Tests

### PostGIS parity tests (behaviour)

Behaviour is tested with the sqllogictest parity suite. Expected output is always recorded from
PostGIS, never written by hand. Use the `postgis-parity-tests` skill
(`.claude/skills/postgis-parity-tests/SKILL.md`). For every new function:

- The generated `slt/postgis_docs/<function>.slt` records for it pass.
- A hand-written `slt/geodatafusion/<function>.slt` covers NULL, EMPTY, every relevant geometry
  type including collections, Z/M, SRID propagation, invalid input, and, for functions with
  several arguments, column/constant combinations of them.
- `parity.txt` is updated with `cargo slt --update-parity`.

Hand-written `.slt` files look like this:

```
# Edge cases for ST_Area, not covered by the PostGIS docs examples.

# EMPTY input
query R
SELECT ST_Area('POLYGON EMPTY'::geometry)
----
0
```

- Start with a one-line `#` comment saying what the file covers. Separate topics with `#`
  section comments.
- One behaviour per query. Uppercase SQL keywords and PostGIS-cased function names
  (`SELECT ST_Area(...)`).
- Use `'...'::geometry` literals and PostGIS's operators as in the PostGIS docs; the harness
  parses in the PostgreSQL dialect.

### Unit tests (Rust API)

Unit tests cover what SQL output can't show: return field types, extension metadata, CRS
propagation, partial aggregation for aggregates
(`target_partitions > 1` with `GROUP BY`), and documentation. They don't repeat behaviour the
`.slt` files cover.

- They live in a `#[cfg(test)] mod test` at the bottom of the file, importing `use super::*;`.
- `#[tokio::test] async fn test_<behaviour>()`. Create a `SessionContext`, register only the UDFs
  under test (plus constructors like `GeomFromText`), and run SQL.
- Every test asserts something. Assert on Arrow arrays (`as_primitive::<Float64Type>()`,
  `as_boolean()`, GeoArrow `value(i)`), and use `approx::relative_eq!` for floats.

## Formatting, linting and compatibility

- Format with `cargo +nightly-2025-05-14 fmt -- --unstable-features --config
  imports_granularity=Module,group_imports=StdExternalCrate`. Imports are grouped as `std`,
  external crates, then `crate`, with one `use` per module.
- `cargo clippy --all-features --tests -- -D warnings` is clean.
- The code compiles on the MSRV (`rust-version` in `Cargo.toml`). Don't use newer standard
  library APIs.
- Prefer existing workspace dependencies. A new dependency goes in `[workspace.dependencies]` in
  the root `Cargo.toml` and is referenced with `{ workspace = true }`.
- Development scripts in `dev/` are formatted and linted with `ruff`.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/) with a capitalised, imperative
  description: `feat: Add ST_Buffer`, `fix: Use topological (recursive) emptiness`,
  `test: Add ST_Area edge cases`, `refactor: Share geometry signature helpers`.
- One function, or one tightly related family (`ST_Point*`), per pull request. Each PR includes
  the implementation, Python binding, tests, the `README.md` table update and `parity.txt`.
- Don't edit `CHANGELOG.md`; it's generated at release time.
