//! ST_Union's aggregate form, and ST_MemUnion.

use std::sync::{Arc, LazyLock};

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::{
    Accumulator, AggregateUDFImpl, Documentation, GroupsAccumulator, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{
    Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
    Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::constant_float_arg;
use crate::util::collect::{
    CollectAccumulator, CollectGroupsAccumulator, Finalize, collect_groups_accumulator_supported,
    collect_state_fields,
};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::{Arg, coerce_args, single_geometry};

/// PostGIS: ST_Union(geometry g1field) and ST_Union(geometry g1field, float8 gridSize), the
/// aggregates.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1field", "gridSize"])
        .expect("parameter names are valid for a user-defined signature")
});

/// The aggregate form of ST_Union.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Aggregate that returns the point-set union of a set of geometries. NULLs are skipped and empty geometries left out; if all are empty, the result is an empty geometry (without Z) of the highest type among them in WKB numbering (POINT, LINESTRING, POLYGON, MULTIPOINT, MULTILINESTRING, MULTIPOLYGON, GEOMETRYCOLLECTION). No geometries give NULL. If gridSize is given (and not negative), the inputs are snapped to a grid of that size and the result is computed on it; it must be a constant, and NULL means none. This function keeps Z and drops M. This is PostGIS's aggregate ST_Union(geometry), named apart from the scalar ST_Union.",
    syntax_example = "ST_Union_Agg(g1field, gridSize)",
    argument(name = "g1field", description = "geometry"),
    argument(name = "gridSize", description = "float8"),
    related_udf(name = "st_union"),
    related_udf(name = "st_memunion")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct UnionAgg;

impl UnionAgg {
    pub fn new() -> Self {
        Self
    }

    /// The final step, with the call's grid size.
    fn finalize(&self, args: &AccumulatorArgs) -> Result<Finalize> {
        // A negative grid size, or NULL, means none, as in PostGIS.
        let grid_size = constant_float_arg(self.name(), args, 1)?
            .flatten()
            .filter(|grid_size| *grid_size >= 0.0);
        Ok(Arc::new(move |geoms| union_agg(geoms, grid_size)))
    }
}

impl Default for UnionAgg {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for UnionAgg {
    fn name(&self) -> &str {
        "st_union_agg"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field should be called instead")
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&arg_fields[0]),
        ))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        let finalize = self.finalize(&args)?;
        Ok(Box::new(CollectAccumulator::try_new(args, finalize)?))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        collect_state_fields(args)
    }

    fn groups_accumulator_supported(&self, args: AccumulatorArgs) -> bool {
        collect_groups_accumulator_supported(args)
    }

    fn create_groups_accumulator(
        &self,
        args: AccumulatorArgs,
    ) -> Result<Box<dyn GroupsAccumulator>> {
        let finalize = self.finalize(&args)?;
        Ok(Box::new(CollectGroupsAccumulator::try_new(args, finalize)?))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The union of a group's geometries, in one GEOS unary union.
fn union_agg(
    geoms: Vec<Wkt<f64>>,
    grid_size: Option<f64>,
) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    let Some(highest_type) = geoms.iter().map(type_number).max() else {
        return Ok(None);
    };
    let members: Vec<Wkt<f64>> = geoms
        .into_iter()
        .filter(|geom| !is_geometry_topologically_empty(geom))
        .collect();
    let Some(dim) = members.first().map(Wkt::dimension) else {
        return Ok(Some(empty_of_type(highest_type)));
    };
    // PostGIS collects the inputs first, which needs the same dimensions, EMPTY ones aside.
    if members.iter().any(|member| member.dimension() != dim) {
        return Err(exec_datafusion_err!("mixed dimension geometries").into());
    }
    // PostGIS keeps a Z from GEOS only when an input has Z.
    let want_z = members.iter().any(has_z);
    let collection = to_geos(&Wkt::GeometryCollection(GeometryCollection::new(
        members, dim,
    )))?;
    let union = match grid_size {
        Some(grid_size) => collection.unary_union_prec(grid_size)?,
        None => collection.unary_union()?,
    };
    Ok(Some(from_geos(&union, want_z)?))
}

/// The geometry's type in WKB numbering.
fn type_number(geom: &Wkt<f64>) -> u8 {
    match geom {
        Wkt::Point(_) => 1,
        Wkt::LineString(_) => 2,
        Wkt::Polygon(_) => 3,
        Wkt::MultiPoint(_) => 4,
        Wkt::MultiLineString(_) => 5,
        Wkt::MultiPolygon(_) => 6,
        Wkt::GeometryCollection(_) => 7,
    }
}

/// An empty 2D geometry of a type in WKB numbering.
fn empty_of_type(type_number: u8) -> Wkt<f64> {
    let dim = Dimension::XY;
    match type_number {
        1 => Wkt::Point(Point::empty(dim)),
        2 => Wkt::LineString(LineString::empty(dim)),
        3 => Wkt::Polygon(Polygon::empty(dim)),
        4 => Wkt::MultiPoint(MultiPoint::empty(dim)),
        5 => Wkt::MultiLineString(MultiLineString::empty(dim)),
        6 => Wkt::MultiPolygon(MultiPolygon::empty(dim)),
        _ => Wkt::GeometryCollection(GeometryCollection::empty(dim)),
    }
}

/// ST_MemUnion.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Aggregate that returns the point-set union of a set of geometries, one geometry at a time: the result so far is unioned with each next geometry as by ST_Union(geom1, geom2), in input order or the call's ORDER BY. So a single geometry is returned unchanged, and an empty one is replaced by the next. The result can differ from ST_Union_Agg's in vertex order and empty geometries. NULLs are skipped, and no geometries give NULL. Here it is no more memory-efficient than ST_Union_Agg: all geometries of a group are held until the end.",
    syntax_example = "ST_MemUnion(geomfield)",
    argument(name = "geomfield", description = "geometry"),
    related_udf(name = "st_union"),
    related_udf(name = "st_union_agg")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MemUnion;

impl MemUnion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemUnion {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for MemUnion {
    fn name(&self) -> &str {
        "st_memunion"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field should be called instead")
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&arg_fields[0]),
        ))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        Ok(Box::new(CollectAccumulator::try_new(
            args,
            Arc::new(mem_union),
        )?))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        collect_state_fields(args)
    }

    fn groups_accumulator_supported(&self, args: AccumulatorArgs) -> bool {
        collect_groups_accumulator_supported(args)
    }

    fn create_groups_accumulator(
        &self,
        args: AccumulatorArgs,
    ) -> Result<Box<dyn GroupsAccumulator>> {
        Ok(Box::new(CollectGroupsAccumulator::try_new(
            args,
            Arc::new(mem_union),
        )?))
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Folds the geometries with ST_Union(geom1, geom2)'s rules.
fn mem_union(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    let mut geoms = geoms.into_iter();
    let Some(mut union) = geoms.next() else {
        return Ok(None);
    };
    for geom in geoms {
        if is_geometry_topologically_empty(&union) {
            union = geom;
        } else if !is_geometry_topologically_empty(&geom) {
            let want_z = has_z(&union) || has_z(&geom);
            union = from_geos(&to_geos(&union)?.union(&to_geos(&geom)?)?, want_z)?;
        }
    }
    Ok(Some(union))
}

#[cfg(test)]
mod test {
    use arrow_array::Array;
    use arrow_array::cast::AsArray;
    use datafusion::prelude::{SessionConfig, SessionContext};

    use super::*;
    use crate::udf::native::io::{AsText, GeomFromText};

    /// A grid size that isn't a constant is a plan error, not a per-row value.
    #[tokio::test]
    async fn test_union_agg_needs_a_constant_grid_size() {
        let ctx = SessionContext::new();
        ctx.register_udaf(UnionAgg.into());
        ctx.register_udf(GeomFromText::default().into());
        let sql =
            "SELECT ST_Union_Agg(ST_GeomFromText('POINT(1 1)'), g) FROM (VALUES (0.5)) AS t(g)";
        let result = match ctx.sql(sql).await {
            Ok(df) => df.collect().await.map(|_| ()),
            Err(e) => Err(e),
        };
        let error = result
            .expect_err("a column grid size is an error")
            .to_string();
        assert!(error.contains("only supports a constant"), "{error}");
    }

    /// A GROUP BY over four partitions, through the groups accumulator, with a grid size.
    #[tokio::test]
    async fn test_union_agg_partitioned() {
        let ctx = SessionContext::new_with_config(SessionConfig::new().with_target_partitions(4));
        ctx.register_udaf(UnionAgg.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(AsText.into());
        let sql = "SELECT k, ST_AsText(ST_Union_Agg(ST_GeomFromText(concat('POINT(', i % 10, '.3 0)')), 1))
            FROM (SELECT i % 2 AS k, i FROM generate_series(0, 199) AS t(i)) GROUP BY k ORDER BY k";
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        let texts: Vec<String> = batches
            .iter()
            .flat_map(|batch| {
                let texts = batch.column(1).as_string::<i32>().clone();
                (0..texts.len()).map(move |row| texts.value(row).to_string())
            })
            .collect();
        assert_eq!(
            texts,
            [
                "MULTIPOINT((0 0),(2 0),(4 0),(6 0),(8 0))",
                "MULTIPOINT((1 0),(3 0),(5 0),(7 0),(9 0))"
            ]
        );
    }
}
