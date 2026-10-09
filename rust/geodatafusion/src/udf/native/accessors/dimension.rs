use std::sync::Arc;

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryCollectionTrait, GeometryTrait, GeometryType};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Returns the topological dimension of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the topological dimension of a geometry: 0 for points, 1 for lines and 2 for polygons. A GEOMETRYCOLLECTION has the largest dimension of its members, including empty ones, and 0 if it has none. An empty geometry has the dimension of its type.",
    syntax_example = "ST_Dimension(g)",
    argument(name = "g", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Dimension;

impl Dimension {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Dimension {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Dimension {
    fn name(&self) -> &str {
        "st_dimension"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dimension_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dimension_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &DimensionKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DimensionKernel;

impl GeometryKernel for DimensionKernel {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        Ok(Some(dimension(geom)))
    }
}

/// The topological dimension of a geometry's type. PostGIS counts empty members of a
/// collection: `GEOMETRYCOLLECTION(POINT(1 1), POLYGON EMPTY)` has dimension 2.
pub(crate) fn dimension(geom: &impl GeometryTrait<T = f64>) -> i32 {
    match geom.as_type() {
        GeometryType::Point(_) | GeometryType::MultiPoint(_) => 0,
        GeometryType::LineString(_) | GeometryType::MultiLineString(_) | GeometryType::Line(_) => 1,
        GeometryType::Polygon(_)
        | GeometryType::MultiPolygon(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_) => 2,
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| dimension(&member))
            .max()
            .unwrap_or(0),
    }
}
