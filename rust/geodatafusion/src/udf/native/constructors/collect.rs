//! ST_Collect, and its aggregate form.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::{
    Accumulator, AggregateUDFImpl, ColumnarValue, Documentation, GroupsAccumulator,
    ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::GeoArrowType;
use wkt::Wkt;
use wkt::types::{GeometryCollection, MultiLineString, MultiPoint, MultiPolygon};

use crate::error::GeoDataFusionResult;
use crate::util::collect::{
    CollectAccumulator, CollectGroupsAccumulator, collect_groups_accumulator_supported,
    collect_state_fields,
};
use crate::util::field::{common_metadata, input_metadata, wkb_return_field};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args, single_geometry};

/// PostGIS: ST_Collect(geometry geom1, geometry geom2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Collects two geometries into one.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Collects two geometries into a geometry collection. Two points, lines or polygons give a MULTIPOINT, MULTILINESTRING or MULTIPOLYGON, anything else a GEOMETRYCOLLECTION; multi-geometries are kept as members, not merged. Empty geometries are kept as members. If one input is NULL, the other is returned unchanged. The inputs must have the same SRID and dimensions. For the aggregate form, see ST_Collect_Agg; the geometry[] form isn't supported.",
    syntax_example = "ST_Collect(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_collect_agg")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Collect;

impl Collect {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Collect {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Collect {
    fn name(&self) -> &str {
        "st_collect"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(collect_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn collect_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geoms1 = OwnedColumn::try_new(&args.args[0], &args.arg_fields[0], args.number_rows)?;
    let geoms2 = OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?;
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(exec_datafusion_err!("{name} returns WKB").into());
    };
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for row in 0..args.number_rows {
        // Unlike most PostGIS functions, ST_Collect isn't STRICT: it returns a non-NULL input
        // as is.
        let result = match (geoms1.get(row), geoms2.get(row)) {
            (Some(geom1), Some(geom2)) => Some(collect(vec![geom1.clone(), geom2.clone()])?),
            (Some(geom), None) | (None, Some(geom)) => Some(geom.clone()),
            (None, None) => None,
        };
        builder.push_geometry(result.as_ref())?;
    }
    Ok(ColumnarValue::Array(builder.finish().to_array_ref()))
}

/// The aggregate form of ST_Collect.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Aggregate that collects a set of geometries into a geometry collection, as ST_Collect does two: points, lines or polygons alone give a MULTIPOINT, MULTILINESTRING or MULTIPOLYGON (even for one geometry), anything else a GEOMETRYCOLLECTION. Members are in input order, or the call's ORDER BY. NULLs are skipped, and no geometries give NULL. This is PostGIS's aggregate ST_Collect(geometry), named apart from the scalar ST_Collect.",
    syntax_example = "ST_Collect_Agg(geom ORDER BY expression)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_collect")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CollectAgg;

impl CollectAgg {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CollectAgg {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for CollectAgg {
    fn name(&self) -> &str {
        "st_collect_agg"
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
        Ok(Box::new(CollectAccumulator::try_new(args, collect_agg)?))
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
            collect_agg,
        )?))
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn collect_agg(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    Ok(Some(collect(geoms)?))
}

/// Collects geometries of the same dimensions into a multi-geometry if they are all points, all
/// lines or all polygons, otherwise into a geometry collection.
pub(crate) fn collect(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Wkt<f64>> {
    let Some(dim) = geoms.first().map(Wkt::dimension) else {
        return Err(exec_datafusion_err!("collect needs at least one geometry").into());
    };
    if geoms.iter().any(|geom| geom.dimension() != dim) {
        return Err(exec_datafusion_err!(
            "Cannot ST_Collect geometries with differing dimensionality."
        )
        .into());
    }
    let collected = match &geoms[0] {
        Wkt::Point(_) if geoms.iter().all(|geom| matches!(geom, Wkt::Point(_))) => {
            Wkt::MultiPoint(MultiPoint::new(
                geoms
                    .into_iter()
                    .filter_map(|geom| match geom {
                        Wkt::Point(point) => Some(point),
                        _ => None,
                    })
                    .collect(),
                dim,
            ))
        }
        Wkt::LineString(_) if geoms.iter().all(|geom| matches!(geom, Wkt::LineString(_))) => {
            Wkt::MultiLineString(MultiLineString::new(
                geoms
                    .into_iter()
                    .filter_map(|geom| match geom {
                        Wkt::LineString(line) => Some(line),
                        _ => None,
                    })
                    .collect(),
                dim,
            ))
        }
        Wkt::Polygon(_) if geoms.iter().all(|geom| matches!(geom, Wkt::Polygon(_))) => {
            Wkt::MultiPolygon(MultiPolygon::new(
                geoms
                    .into_iter()
                    .filter_map(|geom| match geom {
                        Wkt::Polygon(polygon) => Some(polygon),
                        _ => None,
                    })
                    .collect(),
                dim,
            ))
        }
        _ => Wkt::GeometryCollection(GeometryCollection::new(geoms, dim)),
    };
    Ok(collected)
}

#[cfg(test)]
mod test {
    use arrow_array::Array;
    use arrow_array::cast::AsArray;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::{AsText, GeomFromText};

    /// A GROUP BY over enough rows and partitions to go through partial and final aggregation,
    /// which the single-partition slt files don't.
    #[tokio::test]
    async fn test_collect_agg_partitioned() {
        let config = datafusion::prelude::SessionConfig::new().with_target_partitions(4);
        let ctx = SessionContext::new_with_config(config);
        ctx.register_udaf(CollectAgg.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(AsText.into());

        let sql = "SELECT k, ST_AsText(ST_Collect_Agg(ST_GeomFromText(concat('POINT(', i, ' 0)')) ORDER BY i))
            FROM (SELECT i % 3 AS k, i FROM generate_series(0, 299) AS t(i)) GROUP BY k ORDER BY k";
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        let texts: Vec<String> = batches
            .iter()
            .flat_map(|batch| {
                let column = batch.column(1).as_string::<i32>().clone();
                (0..column.len())
                    .map(move |row| column.value(row).to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(texts.len(), 3);
        for (k, text) in texts.iter().enumerate() {
            let expected: Vec<String> = (k..300).step_by(3).map(|i| format!("({i} 0)")).collect();
            assert_eq!(text, &format!("MULTIPOINT({})", expected.join(",")));
        }
    }

    /// Without ORDER BY, a GROUP BY goes through the groups accumulator. The order of members
    /// then depends on the partitioning, so only the set of members is compared.
    #[tokio::test]
    async fn test_collect_agg_groups_accumulator() {
        let config = datafusion::prelude::SessionConfig::new().with_target_partitions(4);
        let ctx = SessionContext::new_with_config(config);
        ctx.register_udaf(CollectAgg.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(AsText.into());

        let sql = "SELECT k, ST_AsText(ST_Collect_Agg(ST_GeomFromText(concat('POINT(', i, ' 0)'))))
            FROM (SELECT i % 3 AS k, i FROM generate_series(0, 299) AS t(i)) GROUP BY k ORDER BY k";
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        let mut groups = 0;
        for batch in &batches {
            let texts = batch.column(1).as_string::<i32>();
            for row in 0..texts.len() {
                let k = groups;
                let text = texts.value(row);
                let members = text
                    .strip_prefix("MULTIPOINT(")
                    .and_then(|rest| rest.strip_suffix(')'))
                    .unwrap_or_else(|| panic!("not a MULTIPOINT: {text}"));
                let mut members: Vec<&str> = members.split(',').collect();
                members.sort_unstable();
                let mut expected: Vec<String> =
                    (k..300).step_by(3).map(|i| format!("({i} 0)")).collect();
                expected.sort_unstable();
                assert_eq!(members, expected);
                groups += 1;
            }
        }
        assert_eq!(groups, 3);
    }

    #[tokio::test]
    async fn test_collect_agg_keeps_crs() {
        let ctx = SessionContext::new();
        ctx.register_udaf(CollectAgg.into());
        ctx.register_udf(GeomFromText::default().into());
        let sql = "SELECT ST_Collect_Agg(ST_GeomFromText('POINT(1 2)', 3857))";
        crate::util::test::assert_wkb_output(&ctx, sql, 3857).await;
    }

    #[test]
    fn test_collect_needs_a_geometry() {
        assert!(collect(Vec::new()).is_err());
    }
}
