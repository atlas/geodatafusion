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
    GeometryCollectionTrait, GeometryTrait, GeometryType, MultiPolygonTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Returns the number of rings in a polygonal geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the number of rings of a polygonal geometry, counting exterior and interior rings. A MULTIPOLYGON or GEOMETRYCOLLECTION has the rings of all its polygons. Geometries without polygons, and empty polygons, have 0.",
    syntax_example = "ST_NRings(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_numinteriorrings")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NRings;

impl NRings {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NRings {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for NRings {
    fn name(&self) -> &str {
        "st_nrings"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(n_rings_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn n_rings_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &NRingsKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct NRingsKernel;

impl GeometryKernel for NRingsKernel {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        let count = n_rings(geom);
        let count = i32::try_from(count)
            .map_err(|_| exec_datafusion_err!("too many rings for an integer: {count}"))?;
        Ok(Some(count))
    }
}

fn n_rings(geom: &impl GeometryTrait<T = f64>) -> usize {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_rings(polygon),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_rings(&polygon))
            .sum(),
        GeometryType::GeometryCollection(collection) => {
            collection.geometries().map(|member| n_rings(&member)).sum()
        }
        GeometryType::Rect(_) | GeometryType::Triangle(_) => 1,
        _ => 0,
    }
}

fn polygon_rings(polygon: &impl PolygonTrait<T = f64>) -> usize {
    match polygon.exterior() {
        Some(_) => 1 + polygon.num_interiors(),
        None => 0,
    }
}
