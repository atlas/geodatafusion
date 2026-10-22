use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use geos::Geom;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::to_geos;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Tests if a LineString is closed and simple.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if the linestring is closed (in 2D) and simple: its ends meet and it doesn't otherwise touch itself. An empty geometry of any type is false; any other geometry but a linestring is an error.",
    syntax_example = "ST_IsRing(g)",
    argument(name = "g", description = "geometry"),
    related_udf(name = "st_issimple")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsRing;

impl IsRing {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsRing {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsRing {
    fn name(&self) -> &str {
        "st_isring"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_ring_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_ring_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsRingKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsRingKernel;

impl GeometryKernel for IsRingKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // PostGIS returns false for any empty geometry before checking the type.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(false));
        }
        if !matches!(geom.as_type(), GeometryType::LineString(_)) {
            return Err(exec_datafusion_err!(
                "ST_IsRing() should only be called on a linear feature"
            )
            .into());
        }
        Ok(Some(to_geos(geom)?.is_ring()?))
    }
}
