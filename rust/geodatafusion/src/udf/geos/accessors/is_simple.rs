use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geos::Geom;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::to_geos;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Tests if a geometry has no points of self-intersection or self-tangency.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if the geometry has no anomalous geometric points, such as self-intersection or self-tangency, as GEOS defines them: a line may only touch itself at its ends, a multipoint has no repeated points, a polygon's rings are simple, and a collection is simple if each member is. An empty geometry is simple.",
    syntax_example = "ST_IsSimple(g)",
    argument(name = "g", description = "geometry"),
    related_udf(name = "st_isvalid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsSimple;

impl IsSimple {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsSimple {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsSimple {
    fn name(&self) -> &str {
        "st_issimple"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_simple_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_simple_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsSimpleKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsSimpleKernel;

impl GeometryKernel for IsSimpleKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // PostGIS answers for an empty geometry without asking GEOS.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(true));
        }
        Ok(Some(to_geos(geom)?.is_simple()?))
    }
}
