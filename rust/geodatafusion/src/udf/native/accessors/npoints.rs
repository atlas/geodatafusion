use std::sync::Arc;

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, MultiLineStringTrait,
    MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the number of points (vertices) in a geometry. Works for all geometries; an empty geometry has 0.",
    syntax_example = "ST_NPoints(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_numpoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NPoints;

impl NPoints {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for NPoints {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for NPoints {
    fn name(&self) -> &str {
        "st_npoints"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(npoints_impl(args, Mode::All)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the number of points in a LINESTRING. Returns NULL for any other geometry type; use ST_NPoints to count the points of any geometry.",
    syntax_example = "ST_NumPoints(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_npoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NumPoints;

impl NumPoints {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NumPoints {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for NumPoints {
    fn name(&self) -> &str {
        "st_numpoints"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(npoints_impl(args, Mode::LineString)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn npoints_impl(args: ScalarFunctionArgs, mode: Mode) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &mode)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    /// ST_NPoints: the points of any geometry.
    All,
    /// ST_NumPoints: the points of a linestring, NULL otherwise.
    LineString,
}

impl GeometryKernel for Mode {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        let count = match (self, geom.as_type()) {
            (Mode::All, _) => num_points(geom),
            (Mode::LineString, GeometryType::LineString(line)) => line.num_coords(),
            (Mode::LineString, GeometryType::Line(_)) => 2,
            (Mode::LineString, _) => return Ok(None),
        };
        let count = i32::try_from(count)
            .map_err(|_| exec_datafusion_err!("too many points for an integer: {count}"))?;
        Ok(Some(count))
    }
}

/// The number of coordinates of a geometry; EMPTY points count 0.
fn num_points(geom: &impl GeometryTrait<T = f64>) -> usize {
    match geom.as_type() {
        GeometryType::Point(point) => usize::from(point.coord().is_some()),
        GeometryType::LineString(line) => line.num_coords(),
        GeometryType::Polygon(polygon) => polygon_points(polygon),
        GeometryType::MultiPoint(points) => points.points().map(|point| num_points(&point)).sum(),
        GeometryType::MultiLineString(lines) => {
            lines.line_strings().map(|line| line.num_coords()).sum()
        }
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_points(&polygon))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| num_points(&member))
            .sum(),
        // As polygons with a closed ring.
        GeometryType::Rect(_) => 5,
        GeometryType::Triangle(_) => 4,
        GeometryType::Line(_) => 2,
    }
}

fn polygon_points(polygon: &impl PolygonTrait<T = f64>) -> usize {
    polygon.exterior().map_or(0, |ring| ring.num_coords())
        + polygon
            .interiors()
            .map(|ring| ring.num_coords())
            .sum::<usize>()
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int32Type;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test() {
        let ctx = SessionContext::new();

        ctx.register_udf(NPoints::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql(
                "select ST_NPoints(ST_GeomFromText('LINESTRING(77.29 29.07,77.42 29.26,77.27 29.31,77.29 29.07)'));",
            )
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Int32Type>().value(0);
        assert_eq!(val, 4);
    }
}
