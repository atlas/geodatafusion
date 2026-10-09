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
    GeometryCollectionTrait, GeometryTrait, GeometryType, MultiLineStringTrait, MultiPointTrait,
    MultiPolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Returns the number of elements in a geometry collection.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the number of members of a collection (MULTI* or GEOMETRYCOLLECTION), and 1 for any other non-empty geometry. An empty geometry, including a collection of empty members, has 0. Otherwise empty members count: MULTIPOINT(EMPTY, (1 1)) has 2.",
    syntax_example = "ST_NumGeometries(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_geometryn")
)]
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

impl ScalarUDFImpl for NumGeometries {
    fn name(&self) -> &str {
        "st_numgeometries"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(num_geometries_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn num_geometries_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &NumGeometriesKernel)?;
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
        // PostGIS counts members structurally, unless the whole geometry is empty.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(0));
        }
        let count = match geom.as_type() {
            GeometryType::MultiPoint(points) => points.num_points(),
            GeometryType::MultiLineString(lines) => lines.num_line_strings(),
            GeometryType::MultiPolygon(polygons) => polygons.num_polygons(),
            GeometryType::GeometryCollection(collection) => collection.num_geometries(),
            _ => 1,
        };
        let count = i32::try_from(count)
            .map_err(|_| exec_datafusion_err!("too many geometries for an integer: {count}"))?;
        Ok(Some(count))
    }
}
