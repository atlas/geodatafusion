# G5: Aggregate, window and set-returning functions

G5 covers every PostGIS function whose DataFusion shape isn't a plain scalar UDF: aggregates
(`AggregateUDFImpl`), window functions (`WindowUDFImpl`) and set-returning functions. The per-row
or per-set *algorithm* usually comes from G1 (native), G2 (`geo`) or G3 (GEOS). G5 owns the UDF
shell, the state handling and the SQL surface. The basis is `bounding_box/extent.rs` (the only
aggregate) and `accessors/dump.rs` (the only set-returning function).

Everything below was checked against DataFusion 54.0.0 (the version in `Cargo.lock`), and the
SQL behaviour claims were tested in a scratch crate against `geodatafusion::register` and against
the PostGIS 3.6.4 server.

> **Reconciled.** This plan was written in parallel with the other group plans. Where it
> conflicts with the cross-group decisions in [README.md](README.md#cross-group-decisions)
> (shared helpers, row access, signatures, argument readers, error and documentation
> conventions, function assignments, output encoding), the README and
> [STYLE_GUIDE.md](../STYLE_GUIDE.md) win. Several of those decisions come from the experiments in
> [hypotheses.md](hypotheses.md), which overturned parts of this plan.

## 1. Scope

### Final function list

| Function | UDF kind | Notes |
|---|---|---|
| ST_Extent | aggregate | Exists. Has bugs, see §2. |
| ST_3DExtent | aggregate | Shares a file and accumulator with ST_Extent. |
| ST_Collect (aggregate form) | aggregate | Registered as `st_collect_agg` for now, see §3 R5. |
| ST_MakeLine (aggregate form) | aggregate | `st_makeline_agg` for now. Order-sensitive (`ORDER BY` inside the call). |
| ST_Union (aggregate forms) | aggregate | `st_union_agg(geom [, gridsize])` for now. GEOS. |
| ST_MemUnion | aggregate | Same result as ST_Union. GEOS. |
| ST_Polygonize (aggregate form) | aggregate | GEOS. Owns the PostGIS name because the scalar `geometry[]` form isn't implemented. |
| ST_CoverageUnion (aggregate form) | aggregate | GEOS. Same naming rule as ST_Polygonize. |
| ST_ClusterIntersecting | aggregate | Returns `geometry[]`. |
| ST_ClusterWithin | aggregate | Returns `geometry[]`. |
| ST_ClusterDBSCAN | window | |
| ST_ClusterKMeans | window | |
| ST_ClusterIntersectingWin | window | |
| ST_ClusterWithinWin | window | |
| ST_CoverageInvalidEdges | window | GEOS >= 3.12. |
| ST_CoverageSimplify | window | GEOS >= 3.12. |
| ST_CoverageClean | window | GEOS >= 3.14. |
| ST_Dump | set-returning | Exists (list-returning scalar). Add the table function form. |
| ST_DumpPoints | set-returning | |
| ST_DumpRings | set-returning | |
| ST_DumpSegments | set-returning | |
| ST_Subdivide | set-returning | Returns `SETOF geometry`. GEOS clipping. |
| ST_SquareGrid | set-returning | Returns `SETOF (geom, i, j)`. |
| ST_HexagonGrid | set-returning | Returns `SETOF (geom, i, j)`. |
| ST_AsMVT, ST_AsGeobuf, ST_AsFlatGeobuf | aggregate | Moved here from G4 (UDF kind decides), deferred, see below. Encoders come from G4. |
| `geometry_dump` (Arrow layout only) | type | The struct layout is defined next to the dump functions. G6 keeps any SQL type name or cast. |

### Reassigned

| Function | From → to | Why |
|---|---|---|
| ST_Collect scalar forms `(geom1, geom2)` and `(geometry[])` | G5 → G1 | Plain native scalars. All 4 doc tests use the scalar forms. The aggregate stays in G5 and reuses the same list kernel (§4). |
| ST_MakeLine scalar forms | G5 → G1 | Same reasoning: all 4 doc tests are scalar. |
| ST_Union scalar forms `(g1, g2 [, gridsize])` and `(geometry[])` | G5 → G3 | GEOS scalar. The only doc test is scalar. |
| ST_AsMVT, ST_AsGeobuf, ST_AsFlatGeobuf | G4 → G5 | They're aggregates. The encoding kernel stays a G4 deliverable. |
| `geometry_dump` | G6 → G5 (layout) | Only the dump functions produce it. |

The scalar and aggregate forms of one PostGIS function live in **one file**, for example
`native/constructors/collect.rs` holding both `Collect` (G1) and `CollectAgg` (G5). Whichever
group lands first writes the shared list kernel.

### Can't be supported yet

- **ST_AsMVT / ST_AsGeobuf / ST_AsFlatGeobuf** take a whole row (`ST_AsGeobuf(q, 'geom') FROM (...) AS q`).
  DataFusion has no "table alias as a row value". An interim `struct(...)` argument would work
  but isn't PostGIS SQL. Deferred until G6 decides on composite-row support.
- **POLYHEDRALSURFACE, TIN, TRIANGLE** inputs: GeoArrow has no such types. That blocks st_dump
  2/2, st_dumppoints 3/4 and st_dumpsegments 2/3 doc tests permanently.
- **`(ST_DumpPoints(geom)).*`** and **`(expr).field`**: DataFusion 54 and 55 reject
  `(expr).ident` with "Dot access not supported for non-string expr", and sqlparser's generic
  dialect doesn't parse `(expr).*`. Upstream fix needed (§5).
- **Set-returning functions in the SELECT list** (`SELECT ST_Subdivide(geom) FROM t` producing
  rows) and **lateral calls** (`FROM t, ST_Dump(t.geom)`). DataFusion only expands `unnest(...)`
  in the SELECT list. `LATERAL (subquery)` plans but fails at execution ("Physical plan does not
  support logical expression OuterReferenceColumn"). Table function arguments are planned against
  an empty schema, so only constant arguments work in `FROM`.
- **ST_ClusterKMeans doc tests** (`query error db error`) can't pass: they expect a PostgreSQL
  error message.
- **ST_FromFlatGeobuf / ST_FromFlatGeobufToTable** (G4) return typed rows or create tables. They're
  not supportable as PostGIS functions. `geodatafusion-flatgeobuf` already covers reading.

## 2. Existing basis

### `rust/geodatafusion/src/udf/native/bounding_box/extent.rs` (ST_Extent)

`Extent` is a unit struct implementing `AggregateUDFImpl` (extent.rs:19-55). The private
`ExtentAccumulator` (63-139) wraps a `BoundingRect` from `bounding_box/util/bounds.rs`. It folds
each batch with `total_bounds` (112-114) and merges four Float64 state columns with
`arrow_arith` min/max (118-134). The result is a `BoxType` XY struct with the input CRS
(57-61, 90-109). It's registered with `register_udaf` (bounding_box/mod.rs:17) and wrapped by a
hand-written `PyExtent` in `python/src/udf/native/bounding_box.rs`.

Issues, verified by running SQL:

1. **No `state_fields`, so it fails whenever partial state is materialised.** The default
   `state_fields` declares one struct column, but `state()` returns four scalars (80-87). With
   `target_partitions = 4`, `SELECT k, ST_Extent(g) ... GROUP BY k` fails with "number of
   columns(5) must match number of fields(2) in schema". An empty filtered input (`... WHERE false`)
   fails with "columns(4) must match ... fields(1)". This is a correctness bug, not style.
2. **Wrong result for no input.** All-NULL or all-EMPTY input returns
   `{xmin: -inf, ymin: -inf, xmax: inf, ymax: inf}`. PostGIS returns NULL.
3. **Two `unwrap()`s on user data** (112-113). They should go through `GeoDataFusionResult`.
4. **No documentation:** no struct doc comment (19) and no `documentation()`.
5. **Fragile input field:** `acc_args.exprs[0].return_field(acc_args.schema)` (52) where
   `acc_args.expr_fields[0]` exists for this purpose.
6. **Inconsistent sentinels:** merging uses `f64::MAX`/`f64::MIN` (119-126) while `BoundingRect` uses
   `±INFINITY`, and it writes `BoundingRect`'s `pub(crate)` fields directly (128-131).
7. **Hard-coded output layout:** `evaluate` repeats the box struct layout by hand (91-108) instead of
   building from the return field's `BoxType`.
8. **Weak tests:** one test named `test`, commented-out imports (143-144), no NULL, empty, GROUP BY
   or multi-partition coverage, and no `.slt`.
9. **Wrong Python stub:** `_bounding_box.pyi` declares `Extent.__datafusion_scalar_udf__` instead of
   `__datafusion_aggregate_udf__`.

The basic shape is right: an accumulator over a small fixed state, and the trait method order
follows the scalar guide. `Accumulator` alone is acceptable. `GroupsAccumulator` is an
optimisation (§3 R1).

### `rust/geodatafusion/src/udf/native/accessors/dump.rs` (ST_Dump)

`Dump` is a **scalar** UDF that returns, per input row, `List<Struct<path: List<Int32>, geom: geometry>>`
(dump.rs:90-112). Users get PostGIS-style rows with `unnest`. `dump_array` (133-171) walks each
geometry with a path stack (175-215), skips topologically empty inputs using
`is_empty::is_geometry_topologically_empty` (23, 149), and assembles the list from a
`GeometryBuilder` plus a path `ListBuilder`.

This model is sound and is the right foundation. Verified:

- `unnest(ST_Dump(g))` yields struct rows, and `d['geom']` **keeps the GeoArrow extension
  metadata**. DataFusion's unnest preserves struct child fields but **drops list item metadata**
  (`get_unnested_columns`, datafusion-expr plan.rs:4401, unchanged in 55). The struct wrapper is
  what makes this work.
- It composes per row (GROUP BY, joins, CTEs), unlike a table function.

Issues:

1. **No table-function form**, so PostGIS's `SELECT path, geom FROM ST_Dump(...)` doesn't work. It
   can be added without new algorithm code (§3 R3).
2. **Private composite-type helpers.** `path_field`, `item_field` and `output_field` (90-112) and the
   build loop are private to the file, but ST_DumpPoints, ST_DumpRings and ST_DumpSegments need
   exactly the same output.
3. **Style drift.** `return_type` uses a custom message (55-57) instead of the guide's
   `"return_type"`. The `return_field_from_args` logic is inline (60-65) rather than in
   `return_field_impl`. Builders aren't created `with_capacity` (137-141).
4. **Documentation.** It names "ST_Empty" (81) where it means ST_IsEmpty, and doesn't follow a
   standard set-returning wording.
5. **Missing pieces.** There's no Python binding, no hand-written `.slt` (PostGIS-compatible
   SQL for it needs the table-function form), and its unit tests use DataFusion-only SQL.

## 3. Refactoring assessment

### R1. Fix and restructure ST_Extent (recommended, S, low risk)

Before (extent.rs:51-54, 80-87):

```rust
fn accumulator(&self, acc_args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
    let input_field = acc_args.exprs[0].return_field(acc_args.schema)?;
    Ok(Box::new(ExtentAccumulator::new(input_field)))
}
// no state_fields: the default declares 1 field, state() returns 4
```

After:

```rust
fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
    Ok(Box::new(ExtentAccumulator::new(
        Arc::clone(&args.expr_fields[0]),
        Arc::clone(&args.return_field),
        self.dim,
    )))
}

fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
    Ok(self
        .dim
        .state_names() // ["xmin", "ymin", "xmax", "ymax"] or the 6 XYZ names
        .iter()
        .map(|n| Field::new(format_state_name(args.name, n), DataType::Float64, true).into())
        .collect())
}
```

- `Extent` and `Extent3D` share the file and accumulator, the way `Box2D`/`Box3D` share
  `box.rs`. A `dim: Dimension` field selects XY or XYZ. ST_3DExtent of 2D input reports z = 0, as
  PostGIS's `BOX3D(1 2 0,1 2 0)` does.
- Add `BoundingRect::is_empty()` and `BoundingRect::merge_state(&[ArrayRef])`, so the accumulator
  stops touching fields and uses the `±INFINITY` sentinels consistently.
- `evaluate` returns `ScalarValue::try_from(return_field.data_type())` (a NULL struct) when empty,
  and otherwise builds one row with `RectBuilder` from the return field's `BoxType`, converting via
  `ScalarValue::try_from_array(&arr, 0)`.
- Errors go through a free `extent_update(...) -> GeoDataFusionResult<()>`.
- Add `documentation()`, the struct doc comment, `order_sensitivity() -> Insensitive` (bounds don't
  depend on order, so DataFusion can skip sorts), a hand-written `slt/geodatafusion/st_extent.slt`,
  and unit tests for GROUP BY with `target_partitions = 4`, empty input and all-NULL input.
- `GroupsAccumulator` (a `Vec<BoundingRect>` indexed by group) is optional. Do it later if
  profiling shows GROUP BY ST_Extent is hot. It's about 120 lines.

### R2. Shared geometry_dump machinery (recommended, S, low risk)

Move the composite type and builder out of dump.rs into
`native/accessors/util/geometry_dump.rs`. That follows the style guide's "helpers shared within
a category live in a private `util` module of that category":

```rust
/// Arrow layout of PostGIS `geometry_dump`: `Struct<path: List<Int32>, geom: geometry>`.
pub(crate) fn geometry_dump_fields(geom_type: &GeometryType) -> Fields;
/// `List<geometry_dump>`, the return type of every ST_Dump* function.
pub(crate) fn geometry_dump_list_field(geom_type: &GeometryType) -> FieldRef;

/// Builds one `List<geometry_dump>` row per input geometry.
pub(crate) struct GeometryDumpBuilder { geoms: GeometryBuilder, paths: ListBuilder<Int32Builder>,
    path: Vec<i32>, offsets: Vec<usize>, validity: NullBufferBuilder }
impl GeometryDumpBuilder {
    pub fn with_capacity(geom_type: GeometryType, len: usize) -> Self;
    pub fn push_path(&mut self, i: i32);   // enter child i (1-based)
    pub fn pop_path(&mut self);
    pub fn push_item(&mut self, geom: &impl GeometryTrait<T = f64>) -> GeoDataFusionResult<()>;
    pub fn finish_row(&mut self);           // close the current input row
    pub fn push_null_row(&mut self);
    pub fn finish(self) -> ListArray;
}
```

ST_Dump becomes a traversal that calls `push_path`/`push_item`. ST_DumpPoints, ST_DumpRings and
ST_DumpSegments are different traversals over the same builder. Fix the style drift listed in §2
in the same PR.

### R3. Set-returning functions: list scalar plus a generic table function (recommended, M, low risk)

How PostgreSQL SQL maps onto DataFusion:

| PostGIS SQL | DataFusion today | Plan |
|---|---|---|
| `SELECT (ST_Dump(g)).geom FROM t` | Not supported (dot access) | `SELECT d['geom'] FROM (SELECT unnest(ST_Dump(g)) AS d FROM t)`. Documented. Upstream fix for `(expr).ident`. |
| `SELECT ST_Dump(g) FROM t` (rows) | Not supported | `SELECT unnest(ST_Dump(g)) FROM t` |
| `FROM ST_Dump(<constant>)` | Works with a UDTF (verified) | Generic adapter, below |
| `FROM t, ST_Dump(t.g)` / `LATERAL` | Not supported | Not possible until DataFusion executes lateral joins |

So the **scalar list-returning UDF stays the core**. Every set-returning function returns
`List<Struct<...>>`. For PostGIS composite types, the struct has PostGIS's column names
(`path`, `geom`; `geom`, `i`, `j`). For `SETOF geometry` (ST_Subdivide), it's a one-field struct
named after the function (`st_subdivide`), which is what PostgreSQL names the column in
`FROM ST_Subdivide(...)`. The struct wrapper is required anyway, because `unnest` of a bare list drops
the GeoArrow metadata.

Then **one generic adapter** exposes any such scalar as a table function of the same name. Table
functions are a separate registry, so they don't clash with scalar UDFs:

```rust
// rust/geodatafusion/src/udf/util/set_returning.rs
/// Exposes a scalar UDF returning `List<Struct<..>>` as a table function, so that
/// `SELECT * FROM ST_Dump(<constant>)` works like the PostGIS set-returning function.
#[derive(Debug)]
pub(crate) struct SetReturningTableFunction {
    udf: Arc<ScalarUDF>,
}

impl TableFunctionImpl for SetReturningTableFunction {
    fn call_with_args(&self, args: TableFunctionArgs) -> Result<Arc<dyn TableProvider>> {
        let call = self.udf.call(args.exprs().to_vec()).alias(SRF_COLUMN);
        // Unnest twice: List<Struct> -> Struct rows -> one column per struct field.
        // `get_field` on the unnested column would be shorter, but DataFusion 54's
        // `push_down_leaf_projections` rule pushes it below the Unnest and fails.
        let plan = LogicalPlanBuilder::empty(true)
            .project(vec![call])?
            .unnest_column(SRF_COLUMN)?
            .unnest_column(SRF_COLUMN)?
            .build()?;
        let columns = plan.schema().columns().into_iter().map(|c| {
            let name = c.name.trim_start_matches(SRF_PREFIX).to_string();
            Expr::Column(c).alias(name)
        });
        let plan = LogicalPlanBuilder::from(plan).project(columns)?.build()?;
        Ok(Arc::new(ViewTable::new(plan, None)))
    }
}

/// Registers a set-returning function as a scalar UDF and as a table function.
pub(crate) fn register_set_returning(ctx: &SessionContext, udf: ScalarUDF) {
    let udf = Arc::new(udf);
    ctx.register_udtf(udf.name(), Arc::new(SetReturningTableFunction { udf: Arc::clone(&udf) }));
    ctx.register_udf(udf.as_ref().clone());
}
```

Verified in the scratch crate:

- `SELECT path, ST_AsText(geom) FROM st_dump(ST_GeomFromText('MULTIPOINT(0 0,1 1)'))` returns
  the PostGIS rows.
- `d.path[1]` and `d.geom` work with an alias, extension metadata survives, and
  `count(*) FROM st_dump('POINT EMPTY')` is 0.
- DataFusion constant-folds the arguments before `call_with_args`
  (`session_state.rs:1977-1992`), so no evaluation happens at planning.

Two DataFusion limitations:

- Table function names **aren't case-normalised** (`relation/mod.rs:156`, same in 55).
  `FROM ST_Dump(...)` fails with "table function 'ST_Dump' not found", while `FROM st_dump(...)` works.
  Hand-written tests use the lowercase spelling in `FROM` (PostgreSQL accepts it), and we file an
  upstream fix. Registering a PostGIS-cased alias as well would pass the ST_DumpRings doc test now
  (open question 3).
- Only constant arguments.

Rejected alternative: an `AnalyzerRule` that turns set-returning scalars in a projection into an
Unnest. It's large (it re-implements `try_process_unnest`), fragile across DataFusion upgrades,
and unlocks only st_subdivide example 1. Revisit if DataFusion gains lateral execution.

Effort: about 80 lines plus tests. Risk is low: no change to existing SQL, and the adapter is
used by every set-returning function.

### R4. Collect-then-finalize aggregates on top of `array_agg` (recommended, M, low-medium risk)

Nearly every G5 aggregate is "collect the group's geometries, then compute once":

- ST_Collect, ST_MakeLine, ST_Union, ST_MemUnion, ST_Polygonize and ST_CoverageUnion.
- ST_ClusterIntersecting and ST_ClusterWithin.
- ST_AsMVT, ST_AsGeobuf and ST_AsFlatGeobuf.

PostGIS implements them the same way: an array-building transition function, then the
`geometry[]` overload as the final function. Hand-rolling accumulators would duplicate state
serialisation, ORDER BY handling, DISTINCT and merging, which are the hard parts. SedonaDB shows
the risk: its `ST_Collect_Agg` ignores `ORDER BY` (apache/sedona-db#1334).

Proposal: **delegate state to DataFusion's own `ArrayAgg`** and only add a finalize step:

```rust
// rust/geodatafusion/src/udf/util/collect.rs
/// Accumulator for aggregates that collect their input and compute the result once per group.
/// State, ORDER BY, DISTINCT and merging are delegated to DataFusion's `array_agg`.
#[derive(Debug)]
pub(crate) struct CollectAccumulator<F> {
    inner: Box<dyn Accumulator>,
    item_field: FieldRef,
    return_field: FieldRef,
    finalize: F,
}

/// One output row per input list, e.g. `|lists, item, ret| collect_lists(lists, item, ret)`.
pub(crate) trait ListFinalize:
    Fn(&ListArray, &Field, &Field) -> GeoDataFusionResult<ArrayRef> + Send + Sync + 'static {}

impl<F: ListFinalize> CollectAccumulator<F> {
    pub fn try_new(args: AccumulatorArgs, finalize: F) -> Result<Self> {
        let item_field = Arc::clone(&args.expr_fields[0]);
        let return_field = Arc::clone(&args.return_field);
        // PostGIS aggregates skip NULL inputs, so drop them before they're stored.
        let inner = ArrayAgg::default().accumulator(AccumulatorArgs { ignore_nulls: true, ..args })?;
        Ok(Self { inner, item_field, return_field, finalize })
    }
}

impl<F: ListFinalize + Debug> Accumulator for CollectAccumulator<F> {
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> { self.inner.update_batch(values) }
    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> { self.inner.merge_batch(states) }
    fn state(&mut self) -> Result<Vec<ScalarValue>> { self.inner.state() }
    fn evaluate(&mut self) -> Result<ScalarValue> {
        let ScalarValue::List(lists) = self.inner.evaluate()? else {
            return internal_err!("array_agg returns a list");
        };
        let result = (self.finalize)(&lists, &self.item_field, &self.return_field)?;
        ScalarValue::try_from_array(&result, 0)
    }
    fn size(&self) -> usize { self.inner.size() }
}

/// `GroupsAccumulator` counterpart: delegates to `array_agg`'s groups accumulator and
/// finalizes the emitted `ListArray` (one row per group) in one call.
pub(crate) struct CollectGroupsAccumulator<F> { /* same fields */ }
// evaluate(emit_to): (self.finalize)(inner.evaluate(emit_to)?.as_list(), ..)

/// The `state_fields`, `groups_accumulator_supported` and `create_groups_accumulator`
/// implementations every collect aggregate uses.
pub(crate) fn collect_state_fields(args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
    ArrayAgg::default().state_fields(args)
}
```

A scratch-crate prototype verified all of the following:

| Scenario | Result |
|---|---|
| `ORDER BY i DESC` inside the call | Respected |
| Empty input | NULL |
| NULL inputs | Skipped |
| GROUP BY with `target_partitions = 4` | Goes through the groups accumulator |
| Ordered GROUP BY | Falls back to the row accumulator |
| `OVER (ORDER BY i)` | Works with no extra code |
| Native GeoArrow union geometry values | Round-trip through `ScalarValue::try_from_array` / `to_array` |

The finalize function **is** the PostGIS `geometry[]` overload, applied to a `ListArray`:

- The scalar `ST_Collect(geometry[])` (G1) calls the same `collect_lists` kernel with its list
  argument.
- `ST_Collect(geom1, geom2)` builds two-element lists, or iterates the pair directly with the
  same per-group helper.

So the G1, G2 or G3 code is reused, not duplicated. The aggregate file contains only the UDF shell.

Risks:

- Memory equals PostGIS's (all geometries of a group are held). ST_MemUnion's incremental
  behaviour isn't reproduced. That's documented, and the result is the same.
- The state type is the input's Arrow type. That's correct, because the accumulator decodes it with
  the remembered input field.
- `ArrayAgg` returns lists without extension metadata. The finalizer always decodes with the
  original input field, so this doesn't matter internally.

### R5. Name overloading between scalar and aggregate forms (recommended interim plus upstream)

Verified: DataFusion's planner resolves a name as scalar UDF first, unconditionally
(datafusion-sql `expr/function.rs:328-329`, same in 55), then window (with OVER), then aggregate.
With both `st_collect` UDF and UDAF registered:

- `SELECT ST_Collect(geom) FROM t GROUP BY k` fails with "Failed to coerce arguments to satisfy a
  call to 'st_collect'".
- No `ExprPlanner` hook intercepts function calls.

| Option | Effect |
|---|---|
| A. Aggregate takes the PostGIS name, no scalar | Loses 9 doc tests and the common `ST_Union(a, b)`. |
| B. Scalar takes the PostGIS name, aggregate gets an `_agg` suffix | Doc tests pass. The aggregate needs a non-PostGIS name. Matches SedonaDB (`ST_Collect_Agg`, `ST_Union_Agg`), Snowflake and DuckDB spatial. |
| C. Upstream: fall back to the UDAF when the scalar's signature can't accept the arguments | The real fix. Embucket's DataFusion fork already does arity-based fallback for exactly `ST_COLLECT` (Embucket/datafusion#91). Arity alone isn't enough here: `ST_Union(geom, gridsize)` aggregate vs `ST_Union(g1, g2)` scalar needs type-based resolution. |

**Recommendation: B now, C upstream.**

- Rule: an aggregate gets the PostGIS name unless geodatafusion also implements a same-named
  scalar form. In that case it's registered as `st_<name>_agg` and documented as
  "PostGIS's aggregate `ST_Name(geometry)`".
- That gives `st_collect_agg`, `st_makeline_agg` and `st_union_agg`. ST_Polygonize,
  ST_CoverageUnion, ST_ClusterIntersecting, ST_ClusterWithin, ST_Extent and ST_3DExtent keep their
  names. Their rarely used `geometry[]` scalar overloads are left out and documented.
- When C lands, register the aggregates under the PostGIS names and keep `_agg` as an alias.

Hand-written tests for `_agg` names must still be recorded on PostGIS. A prelude in the harness's
PostGIS engine works and keeps PostGIS as the oracle: it defines session-scoped aliases built
from PostGIS's own array overloads.

```sql
CREATE AGGREGATE pg_temp.st_collect_agg(geometry)
  (SFUNC = array_append, STYPE = geometry[], FINALFUNC = st_collect);
```

Verified identical to `ST_Collect(g ...)` for ORDER BY, NULL inputs, empty input and ST_Union.
This touches `tests/sqllogictests/postgis.rs`, see open question 1.

### R6. Python bindings for aggregates and window functions (recommended, S, low risk)

`python/src/utils.rs` only generates `__datafusion_scalar_udf__` wrappers (35-98). `PyExtent` is
written out by hand (`python/src/udf/native/bounding_box.rs:28-50`), and its `.pyi` stub declares
the wrong dunder. ST_Dump isn't exposed at all.

Proposal: generate every kind from one private macro, keeping the existing public macro names.

```rust
// python/src/utils.rs
macro_rules! __impl_py_function {
    ($base:ident, $py:ident, $name:literal, $dunder:ident, $udf:ty, $ffi:ty, $capsule:expr,
     ($($arg:ident: $ty:ty),*), $ctor:expr) => {
        #[::pyo3::pyclass(module = "geodatafusion", name = $name, frozen)]
        #[derive(Debug, Clone)]
        pub struct $py(::std::sync::Arc<$base>);

        #[::pyo3::pymethods]
        impl $py {
            #[new]
            #[pyo3(signature = (*, $($arg=None),*))]
            fn new($($arg: $ty),*) -> Self { $py(::std::sync::Arc::new($ctor)) }

            fn $dunder<'py>(&self, py: ::pyo3::Python<'py>)
                -> ::pyo3::PyResult<::pyo3::Bound<'py, ::pyo3::types::PyCapsule>> {
                let udf = ::std::sync::Arc::new(<$udf>::new_from_shared_impl(self.0.clone()));
                ::pyo3::types::PyCapsule::new(py, <$ffi>::from(udf), Some($capsule.into()))
            }
        }
    };
}
// impl_udf!, impl_udf_coord_type_arg!      -> ScalarUDF,    FFI_ScalarUDF,    "datafusion_scalar_udf"
// impl_udaf!, impl_udaf_coord_type_arg!    -> AggregateUDF, FFI_AggregateUDF, "datafusion_aggregate_udf"
// impl_udwf!, impl_udwf_coord_type_arg!    -> WindowUDF,    FFI_WindowUDF,    "datafusion_window_udf"
```

- `FFI_WindowUDF: From<Arc<WindowUDF>>` exists (datafusion-ffi 54 `udwf/mod.rs:224`), and
  datafusion-python reads `__datafusion_window_udf__` (`udwf.rs:265-273`).
- Add `WINDOW_UDF_CAPSULE_NAME` to `constants.rs`, fix the Extent stub, register with
  `ctx.register_udwf(udwf(...))` in `register_all_*`, and expose `Dump` (list form) via
  `impl_udf_coord_type_arg!`.
- Table functions over FFI need `FFI_TableFunction::new(udtf, runtime, task_ctx_provider, codec)`
  with the session passed into `__datafusion_table_function__(session)`. Defer (open question 5).
  Python users can use `unnest(ST_Dump(...))`.

### R7. `Extent` and other aggregates take `coord_type` where they return geometry

Aggregates returning geometry (Collect, Union, ...) store `coord_type: CoordType` with
`new(coord_type)` plus `Default`, exactly like scalar geometry UDFs. ST_Extent and ST_3DExtent
return boxes and stay unit structs. No change to existing code, but it's stated in the style guide.

### Summary

| # | Refactor | Effort | Risk | Recommendation |
|---|---|---|---|---|
| R1 | Fix and restructure ST_Extent, add ST_3DExtent | S | Low | Do first: it's a live bug |
| R2 | Shared geometry_dump builder, dump.rs style fixes | S | Low | Do |
| R3 | `SetReturningTableFunction` and `register_set_returning` | M | Low | Do |
| R4 | `CollectAccumulator` on `ArrayAgg` | M | Low-med | Do before any new aggregate |
| R5 | `_agg` interim naming, upstream overload PR, PostGIS prelude | S (+ upstream) | Low | Do |
| R6 | Python `impl_udaf!`/`impl_udwf!` | S | Low | Do |

## 4. Canonical templates

### Shared helpers to add

| Item | Location | Purpose |
|---|---|---|
| `CollectAccumulator<F>`, `CollectGroupsAccumulator<F>`, `collect_state_fields` | `src/udf/util/collect.rs` | R4 |
| `SetReturningTableFunction`, `register_set_returning` | `src/udf/util/set_returning.rs` | R3 |
| `literal_arg(exprs: &[Arc<dyn PhysicalExpr>], i: usize, fn_name: &str) -> GeoDataFusionResult<ScalarValue>` | `src/udf/util/literal.rs` | Constant arguments of aggregates and windows (gridsize, distance, eps, k). Evaluates against an empty batch, like `datafusion-functions-aggregate`'s `get_scalar_value` (utils.rs:30), so folded casts also work. Errors with `Plan("ST_ClusterWithin only supports a constant distance")` otherwise. |
| `geometry_dump_fields`, `GeometryDumpBuilder` | `src/udf/native/accessors/util/geometry_dump.rs` | R2 |
| `union_find_clusters(geoms, predicate) -> Vec<Option<u32>>` | `src/udf/geo/clustering/util.rs` | One clustering kernel for Intersecting/Within, aggregate and window |

`src/udf/util/` is new, crate-private (`pub(crate) mod util;` in `udf/mod.rs`), and holds
machinery that spans providers. Coordinate with G6, which owns "UDF scaffolding". If G6 creates
an equivalent location, use it.

### Aggregate (collect-then-finalize)

In `native/constructors/collect.rs`, next to G1's scalar `Collect`:

```rust
/// Aggregate that creates a GeometryCollection or Multi* geometry from a set of geometries.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CollectAgg {
    coord_type: CoordType,
}

impl CollectAgg {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for CollectAgg {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

static COLLECT_AGG_DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl AggregateUDFImpl for CollectAgg {
    fn name(&self) -> &str {
        "st_collect_agg"
    }

    fn signature(&self) -> &Signature {
        any_single_geometry_type_input()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("return_type".to_string()))
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        Ok(return_field_impl(arg_fields, self.coord_type)?)
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        Ok(Box::new(CollectAccumulator::try_new(args, collect_lists)?))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        collect_state_fields(args)
    }

    fn groups_accumulator_supported(&self, args: AccumulatorArgs) -> bool {
        collect_groups_accumulator_supported(args)
    }

    fn create_groups_accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn GroupsAccumulator>> {
        Ok(Box::new(CollectGroupsAccumulator::try_new(args, collect_lists)?))
    }

    fn documentation(&self) -> Option<&Documentation> {
        Some(COLLECT_AGG_DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Aggregate that creates a GeometryCollection or Multi* geometry from a set of \
                 geometries. This is PostGIS's aggregate ST_Collect(geometry); it has a different \
                 name because ST_Collect is the two-argument scalar function here.",
                "ST_Collect_Agg(geom [ORDER BY expression])",
            )
            .with_argument("geom", "geometry")
            .with_related_udf("st_collect")
            .build()
        }))
    }
}

/// Collects each list of geometries into one geometry, as PostGIS's `ST_Collect(geometry[])`.
/// Shared by the scalar and aggregate forms.
pub(crate) fn collect_lists(
    lists: &ListArray,
    item_field: &Field,
    return_field: &Field,
) -> GeoDataFusionResult<ArrayRef> { ... }
```

Method order is DataFusion's: `name`, `aliases`, `signature`, `return_type`, `return_field`,
`accumulator`, `state_fields`, `groups_accumulator_supported`, `create_groups_accumulator`,
`order_sensitivity`, `documentation`. Order-insensitive aggregates (Union, Polygonize,
CoverageUnion, Cluster*, Extent) return `AggregateOrderSensitivity::Insensitive`. Collect and
MakeLine keep the default hard requirement, so `ORDER BY` inside the call is honoured. Constant
parameters are read in `accumulator` with `literal_arg(args.exprs, 1, "ST_Union")` and captured
by the finalize closure.

### Aggregate (streaming state)

ST_Extent and ST_3DExtent use a hand-written `Accumulator` with explicit `state_fields` (§3 R1).
Use this shape only when the state is small and fixed-size.

### Window function

In `geo/clustering/cluster_dbscan.rs`:

```rust
/// Window function that returns a cluster id for each input geometry using the DBSCAN algorithm.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ClusterDBSCAN;

impl ClusterDBSCAN {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ClusterDBSCAN { fn default() -> Self { Self::new() } }

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| /* geometry, Float64, Int64 */);
static DOCUMENTATION: OnceLock<Documentation> = OnceLock::new();

impl WindowUDFImpl for ClusterDBSCAN {
    fn name(&self) -> &str {
        "st_clusterdbscan"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn partition_evaluator(&self, args: PartitionEvaluatorArgs) -> Result<Box<dyn PartitionEvaluator>> {
        let eps = literal_f64(args.input_exprs(), 1, "ST_ClusterDBSCAN")?;
        let min_points = literal_i64(args.input_exprs(), 2, "ST_ClusterDBSCAN")?;
        let field = Arc::clone(&args.input_fields()[0]);
        Ok(Box::new(ClusterDBSCANEvaluator { field, eps, min_points }))
    }

    fn field(&self, field_args: WindowUDFFieldArgs) -> Result<FieldRef> {
        // PostGIS returns integer, NULL for noise and NULL input.
        Ok(Field::new(field_args.name(), DataType::Int32, true).into())
    }

    fn documentation(&self) -> Option<&Documentation> { ... }
}

#[derive(Debug)]
struct ClusterDBSCANEvaluator { field: FieldRef, eps: f64, min_points: i64 }

impl PartitionEvaluator for ClusterDBSCANEvaluator {
    // The cluster ids depend on the whole partition, never on the window frame, so the
    // default `uses_window_frame() == false` makes DataFusion call `evaluate_all` once.
    fn evaluate_all(&mut self, values: &[ArrayRef], num_rows: usize) -> Result<ArrayRef> {
        Ok(cluster_dbscan_impl(&values[0], &self.field, self.eps, self.min_points, num_rows)?)
    }
}
```

- Method order: `name`, `aliases`, `signature`, `partition_evaluator`, `field`, `documentation`
  (as in `datafusion-functions-window`'s `ntile.rs`).
- Window functions returning geometry (coverage functions) store `coord_type` and build
  `field` from `field_args.input_fields()[0]` metadata.
- Coverage evaluators collect the partition into one GEOS collection, run the coverage
  operation and split the result back per row.
- Aggregates need no window code. Every UDAF works with `OVER (...)` (verified for ST_Extent).

### Set-returning function

In `native/accessors/dump_points.rs`:

```rust
/// Returns a set of geometry_dump rows for the coordinates in a geometry.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DumpPoints { coord_type: CoordType }
// new / Default as usual

impl ScalarUDFImpl for DumpPoints {
    fn name(&self) -> &str { "st_dumppoints" }
    fn signature(&self) -> &Signature { any_single_geometry_type_input() }
    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("return_type".to_string()))
    }
    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_dump_return_field(args, self.coord_type)?)   // shared with ST_Dump
    }
    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dump_points_impl(args, self.coord_type)?)
    }
    fn documentation(&self) -> Option<&Documentation> {
        Some(DOCUMENTATION.get_or_init(|| {
            Documentation::builder(
                DOC_SECTION_OTHER,
                "Returns a set of geometry_dump rows for the coordinates in a geometry. \
                 As a table function (`SELECT path, geom FROM ST_DumpPoints(...)`) it takes \
                 constant arguments. Per row, it returns a list of `(path, geom)` structs; use \
                 `unnest(ST_DumpPoints(geom))` to expand it into rows.",
                "ST_DumpPoints(geom)",
            )
            .with_argument("geom", "geometry")
            .with_related_udf("st_dump")
            .build()
        }))
    }
}

fn dump_points_impl(args: ScalarFunctionArgs, coord_type: CoordType) -> GeoDataFusionResult<ColumnarValue> {
    // values_to_arrays, from_arrow_array, downcast_geoarrow_array!(.., impl_dump_points, ..)
    // with a GeometryDumpBuilder::with_capacity(geom_type, len)
}
```

Registration in the category `mod.rs`:

```rust
pub fn register(session_context: &SessionContext) {
    register_set_returning(session_context, Dump::default().into());
    register_set_returning(session_context, DumpPoints::default().into());
    ...
}
```

## 5. Dependencies

### DataFusion and crate APIs (verified in 54.0.0)

| API | Location | Use |
|---|---|---|
| `AggregateUDFImpl::{return_field, accumulator, state_fields, groups_accumulator_supported, create_groups_accumulator, order_sensitivity, documentation}` | datafusion-expr `udaf.rs:514-899` | Aggregates |
| `AccumulatorArgs { return_field, expr_fields, exprs, order_bys, ignore_nulls, .. }` (all pub) | datafusion-functions-aggregate-common `accumulator.rs:29-74` | Input field, constant args, `ignore_nulls` override |
| `StateFieldsArgs`, `format_state_name` | datafusion-expr | ST_Extent state |
| `datafusion::functions_aggregate::array_agg::ArrayAgg` (`Default`, `accumulator`, `state_fields`, `create_groups_accumulator`) | not feature-gated in `datafusion` (lib.rs:879) | R4 |
| `ScalarValue::try_from_array` for dense-union geometries | | Verified round-trip |
| `WindowUDFImpl::{partition_evaluator, field}`, `PartitionEvaluator::evaluate_all`, `PartitionEvaluatorArgs::{input_exprs, input_fields}`, `WindowUDFFieldArgs` | datafusion-expr `udwf.rs:315-432`, re-exported from `datafusion::logical_expr::function` | Windows |
| `TableFunctionImpl::call_with_args`, `TableFunctionArgs::exprs` (`call` is deprecated) | datafusion-catalog `table.rs:526-568` | R3 |
| `SessionContext::register_udtf`, `datafusion::datasource::ViewTable`, `LogicalPlanBuilder::{empty, project, unnest_column}` | | R3 |
| `FFI_AggregateUDF`, `FFI_WindowUDF` | datafusion-ffi 54 | R6 |
| `geos::Geom::{unary_union, unary_union_prec, coverage_union}`, `Geometry::polygonize`, `Geometry::create_*` | geos 11.1 | Union, CoverageUnion, Polygonize |
| `GEOSCoverageIsValid_r` / `GEOSCoverageSimplifyVW_r` (3.12), `GEOSCoverageClean*` (3.14) | geos-sys only | No safe `geos` wrapper |

### Upstream DataFusion changes to file

Each is small and independently useful. Workarounds are in place until they land.

1. Plan `(expr).ident` as a named field access. `AccessExpr::Dot(SQLExpr::Identifier)` in
   `expr/mod.rs:1216`.
2. Normalise table function names (`relation/mod.rs:156`).
3. Preserve list item field metadata in `unnest` (`get_unnested_columns`, `plan.rs:4401`).
   Blocks `unnest(ST_ClusterIntersecting(...))`, `unnest(ARRAY['..'::geometry])` and any bare
   `SETOF geometry`.
4. Preserve item metadata in `make_array` / `array_agg` return fields (verified dropped). Blocks
   `ST_Collect(ARRAY[...])` and `ST_MakeLine(ARRAY[...])` doc tests (G1).
5. Fall back from a scalar UDF to a same-named UDAF when the scalar can't accept the argument
   types (R5).
6. `push_down_leaf_projections` pushes `get_field` below `Unnest` (verified failure). Worked around
   in R3.

### Other groups

| Group | Provides |
|---|---|
| G1 | Scalar ST_Collect and ST_MakeLine with the list kernels (`collect_lists`, `make_line_lists`). ST_Square and ST_Hexagon cell kernels for the grids. ST_Dump traversal semantics for DumpPoints, DumpRings and DumpSegments. |
| G2 | Euclidean distance and intersects predicates for clustering. A `geo` upgrade to 0.33 would bring `KMeans`, but PostGIS's k-means++ seeding differs, so plan a native port. |
| G3 | GEOS kernels: unary union (+ gridsize), polygonize, coverage union, coverage validity/simplify/clean (new `geos-3_12` and `geos-3_14` features), and clip-by-rect for ST_Subdivide. Scalar ST_Union. |
| G4 | Geobuf, FlatGeobuf and MVT encoders (deferred aggregates). |
| G6 | Signature helpers for `geometry + Float64 (+ Int64)` arguments. The `geom::geometry` column cast (blocks the `*Win` doc tests). Composite-row arguments (blocks ST_AsMVT etc.). Location of crate-wide UDF scaffolding (`udf/util`). The PostGIS prelude in the harness (R5). |

## 6. Phasing

Batch 0 is refactors. Each later batch is one PR per function or tight family, following the
style guide.

| Batch | Contents | Doc tests unlocked | Depends on |
|---|---|---|---|
| 0a | R1: ST_Extent fix and ST_3DExtent; `st_extent.slt`, `st_3dextent.slt` | — (no doc examples) | — |
| 0b | R2 + R3: geometry_dump builder, `SetReturningTableFunction`, ST_Dump table form, `st_dump.slt` | — | — |
| 0c | R4 + `literal_arg`: collect machinery, with ST_Collect_Agg as first user | — | G1 `collect_lists` (or write it here) |
| 0d | R6: Python macros, Extent stub fix, Dump binding | — | — |
| 0e | Style guide amendments (§9). File upstream issues 1-6. PostGIS prelude (R5). | — | G6 / maintainer |
| 1 | ST_MakeLine_Agg, ST_Union_Agg (+ gridsize), ST_MemUnion, ST_Polygonize, ST_CoverageUnion | st_polygonize 1, st_coverageunion 1 | G3 kernels (`geos-3_11`) |
| 2 | ST_DumpPoints, ST_DumpRings, ST_DumpSegments; ST_SquareGrid, ST_HexagonGrid | st_dumprings 1 (needs upstream 2 or open question 3) | G1 ST_Square/ST_Hexagon |
| 3 | Clustering kernel; ST_ClusterIntersectingWin, ST_ClusterWithinWin, ST_ClusterDBSCAN, ST_ClusterIntersecting, ST_ClusterWithin, ST_ClusterKMeans | 4 once G6 cast and upstream 3 land | G2 predicates, G6, upstream 3 |
| 4 | ST_CoverageInvalidEdges, ST_CoverageSimplify (GEOS 3.12), ST_CoverageClean (GEOS 3.14) | 2 | G3 geos-sys wrappers, CI GEOS version |
| 5 | Deferred: ST_Subdivide, ST_AsMVT/ST_AsGeobuf/ST_AsFlatGeobuf, Python table functions, rename `_agg` to PostGIS names | st_subdivide 0/2 remains (select-list SRF, geography) | upstream 5, G4, G6 |

Batch 1 comes before the set-returning functions because ST_Union and ST_Collect aggregates are
the most used G5 functions and are GEOS-ready today. Batch 3 has the most doc tests but is
blocked outside G5.

## 7. Per-function notes

| Function | Kind | Per-row algorithm | PostGIS gotchas | Diff. | Doc tests |
|---|---|---|---|---|---|
| ST_Extent | agg (stream) | native `BoundingRect` | NULL for no rows / all NULL / all EMPTY. Box has no SRID. | S | — |
| ST_3DExtent | agg (stream) | native | 2D input gives z = 0 (`BOX3D(1 2 0,1 2 0)`) | S | — |
| ST_Collect_Agg | agg (collect) | G1 `collect_lists` | Mixed types give GEOMETRYCOLLECTION. Keeps duplicates. Mixed SRIDs error. EMPTY members kept (`MULTIPOINT(EMPTY,(1 2))`, may not be representable in GeoArrow). ORDER BY respected. | S | 0/4 (all scalar, G1) |
| ST_MakeLine_Agg | agg (collect) | G1 `make_line_lists` | Order-sensitive. Points, MultiPoints and LineStrings accepted. Repeated junction points dropped. | S | 0/4 (all scalar, G1) |
| ST_Union_Agg | agg (collect) | G3 unary union | NULLs ignored. `gridsize` constant. Union of a single point returns POINT. | M | 0/1 (scalar, G3) |
| ST_MemUnion | agg (collect) | G3 unary union | Same result as ST_Union. Not memory-efficient here (documented). | S | — |
| ST_Polygonize | agg (collect) | G3 polygonize | Returns GEOMETRYCOLLECTION of polygons. | S | 0/1 |
| ST_CoverageUnion | agg (collect) | G3 coverage_union | Invalid coverage gives an invalid result, no error. | S | 0/1 |
| ST_ClusterIntersecting | agg (collect) | union-find + intersects (G2) | Returns `geometry[]` of GEOMETRYCOLLECTIONs. NULL for no rows. Doc test needs upstream 3 twice (CTE input and output). | M | 0/1 |
| ST_ClusterWithin | agg (collect) | union-find + distance (G2) | As above, with `distance` constant | M | 0/1 |
| ST_ClusterIntersectingWin | window | shared kernel | 0-based ids in order of first appearance. NULL geometry gives NULL. | M | 0/1 (needs G6 cast) |
| ST_ClusterWithinWin | window | shared kernel | As above | S | 0/1 (needs G6 cast) |
| ST_ClusterDBSCAN | window | native (R-tree + distance) | Noise is NULL. `minpoints` counts the point itself. Border points go to the first cluster that reaches them. Any geometry type. | M | — |
| ST_ClusterKMeans | window | native k-means++ port | `max_radius` optional. EMPTY/NULL geometries get NULL. Results depend on seeding, so match PostGIS's deterministic init. | L | 0/2 (unattainable) |
| ST_CoverageInvalidEdges | window | G3 GEOS 3.12 | NULL for valid polygons. `tolerance` default 0. | M | 0/1 |
| ST_CoverageSimplify | window | G3 GEOS 3.12 | `simplifyboundary` default true | M | 0/1 |
| ST_CoverageClean | window | G3 GEOS 3.14 | Text `overlapmergestrategy` | M | — |
| ST_Dump | SRF | native (exists) | 0 rows for EMPTY. Atomic input gets path `{}`. SRID kept on parts. | S | 0/2 (unattainable: TIN/polyhedral) |
| ST_DumpPoints | SRF | native traversal | Paths: POINT `{1}`, MULTIPOINT `{i,1}`, LINESTRING `{i}`, POLYGON `{ring,i}`, collections prepend member index | S | 0/4 (unattainable: `.*`, TIN, TRIANGLE) |
| ST_DumpRings | SRF | native | Polygon input only ("Input is not a polygon" for MULTIPOLYGON). Exterior path `{0}`, holes `{1..}`. Rings returned as POLYGONs. | S | 0/1 |
| ST_DumpSegments | SRF | native | Segments as 2-point LINESTRINGs. POINT gives 0 rows. | S | 0/3 (unattainable) |
| ST_SquareGrid | SRF | G1 ST_Square | Columns `geom, i, j`. Cells cover the bounds' extent, including touching cells (a 2x2 box with size 1 gives 3x3 cells). | M | — |
| ST_HexagonGrid | SRF | G1 ST_Hexagon | Same column layout | M | — |
| ST_Subdivide | SRF | PostGIS's recursive box split, G3 clip | Output must replicate PostGIS's split points to match. `gridsize` arg. Column named `st_subdivide`. | L | 0/2 |
| ST_AsMVT / ST_AsGeobuf / ST_AsFlatGeobuf | agg (collect) | G4 encoders | Row-valued argument. Deferred. | L | 0/1 (st_asgeobuf) |

## 8. Testing

- **Writing PostgreSQL-compatible SQL.** Hand-written `.slt` files are recorded on PostGIS, so
  every query must run on both engines.
  - Aggregates use inline data: `SELECT ST_AsText(ST_Union_Agg(geom)) FROM (VALUES ('...'::geometry), (...)) AS t(geom)`.
    The `_agg` names need the PostGIS prelude (R5).
  - Set-returning functions use the table form with a constant argument and a lowercase name:
    `SELECT path, ST_AsText(geom) FROM st_dumppoints('...'::geometry)`. Paths render as `{1,2}` on both
    sides.
  - Window functions use `VALUES` with literals and `OVER ()` or `OVER (PARTITION BY ...)`.
- **Ordering.** Multi-row results without `ORDER BY` are compared with `rowsort`. Window function
  ids depend on input order, so give the window an `ORDER BY` (`OVER (ORDER BY id)`) whenever ids
  are asserted. For ST_Collect_Agg and ST_MakeLine_Agg, always test `ORDER BY` inside the call.
- **Cases every aggregate covers:**
  - Empty input (`WHERE false`) gives NULL.
  - All-NULL input gives NULL. Mixed NULL input skips the NULLs.
  - EMPTY members.
  - A single row.
  - GROUP BY with several groups, including a group whose rows are all NULL.
  - Mixed geometry types, Z/M dimensions and SRID propagation.
  - Use as a window (`OVER ()`).
- **Partitioning.** Unit tests for every aggregate run a GROUP BY with
  `SET datafusion.execution.target_partitions = 4` over enough rows to force partial/final
  aggregation. ST_Extent's current bug only shows up there. `slt` runs single-partition VALUES,
  so it doesn't catch this.
- **Window cases:** NULL geometry, a single-row partition, multiple partitions, and constant-argument
  validation (`ST_ClusterWithinWin(geom, col)` gives a `Plan` error).
- **Set-returning cases:** NULL gives 0 rows, EMPTY gives 0 rows, each geometry type including
  nested collections, Z/M, SRID kept on output geometries, and both the table form and
  `unnest(...)` form in unit tests.
- **Unit tests** additionally check `state_fields` and output field metadata (CRS, `coord_type`).

## 9. Style guide amendments

1. **Replace the "Aggregates" bullet in "Anatomy of a UDF"** with a short "Other UDF kinds"
   section. It points to the three templates in §4 and states the method orders:
   - Aggregates: `name`, `aliases`, `signature`, `return_type`, `return_field`, `accumulator`,
     `state_fields`, `groups_accumulator_supported`, `create_groups_accumulator`,
     `order_sensitivity`, `documentation`.
   - Windows: `name`, `aliases`, `signature`, `partition_evaluator`, `field`, `documentation`.

   Why: the current bullet points at extent.rs, which has the bugs listed in §2.
2. **Aggregates must implement `state_fields`** whenever `state()` isn't exactly the return value,
   and must be unit-tested with partial aggregation (`target_partitions > 1`, GROUP BY). Why: the
   default silently mismatches and only fails in multi-partition plans.
3. **Collect-style aggregates use `CollectAccumulator`/`CollectGroupsAccumulator`, never hand-rolled
   geometry state.** The finalize function is the `geometry[]` kernel, shared with the scalar form
   in the same file. Why: one implementation of ordering, NULLs and merging, and reuse of G1-G3
   kernels.
4. **Constant parameters of aggregates and windows** are read once with `literal_arg` and
   rejected with `Plan` errors if not constant. Why: that's DataFusion's convention (`ntile`,
   `approx_percentile_cont`).
5. **Set-returning functions** are scalar UDFs returning `List<Struct<..>>`, with PostGIS's column
   names, registered with `register_set_returning`. Documentation says how to use the `unnest` and
   table forms.
6. **Naming rule for scalar/aggregate pairs** (R5): `st_<name>_agg` when geodatafusion implements
   a same-named scalar, otherwise the PostGIS name. The `ST_Collect` scalar and `ST_Collect_Agg`
   share `collect.rs`. Add an "Aggregate struct: `<Struct>Agg`" row to the naming table, with
   `<STRUCT>_AGG_DOCUMENTATION`.
7. **Tests:** allow lowercase table-function names in `FROM` (DataFusion doesn't normalise them)
   and the `_agg` names. Require `ORDER BY` in `OVER` when cluster ids are asserted.
8. **Python bindings:** add `impl_udaf!`/`impl_udwf!` (and `_coord_type_arg` variants), register
   with `ctx.register_udaf(udaf(...))` / `ctx.register_udwf(udwf(...))`, and use the right dunder in
   stubs. Set-returning functions are exposed as their scalar list form.
9. **Errors:** add to "Errors" that an `internal_err!` / `Internal` error is right only for
   "DataFusion returned an unexpected shape" (for example `array_agg` not returning a list), never for
   user input.

## 10. Open questions for the maintainer

1. **PostGIS prelude in the harness.** May `tests/sqllogictests/postgis.rs` create session-scoped
   alias aggregates (`pg_temp.st_collect_agg`, ...) so that `_agg` tests can be recorded? The
   alternative is no hand-written `.slt` for those aggregates until upstream overloading lands.
2. **Interim `_agg` names.** Are `st_collect_agg`, `st_makeline_agg` and `st_union_agg` acceptable
   until DataFusion supports scalar/UDAF overloading? Should we drive the upstream PR (open
   question 6)?
3. **Mixed-case table function alias.** Should we also register `ST_DumpRings`-cased table function
   names, which passes the doc test before upstream normalisation? It's a hack and only covers that
   exact casing.
4. **CI GEOS version** for the coverage window functions (3.12, and 3.14 for ST_CoverageClean).
   Should we use `geos-sys` directly or contribute safe wrappers to the `geos` crate?
5. **Python table functions.** Expose them now (needs `FFI_TableFunction` with a task context
   provider from the session capsule), or defer and document `unnest`?
6. **Upstream work.** Six small DataFusion issues are listed in §5. Should G5 file them all, or is
   someone already tracking `unnest`/`make_array` metadata loss for extension types?
7. **ST_AsMVT and friends.** Accept a `struct(...)` argument as an interim non-PostGIS signature,
   or wait for row-valued arguments?
8. **ST_Subdivide.** Is exact output parity worth porting PostGIS's recursive subdivision, or is
   "same coverage, different pieces" acceptable and documented?
