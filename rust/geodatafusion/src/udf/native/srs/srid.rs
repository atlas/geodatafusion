use std::sync::Arc;

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// Returns the spatial reference identifier for a geometry.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Returns the spatial reference identifier for a geometry. geodatafusion stores one CRS per column, so every row has the column's SRID. A CRS that doesn't name an SRID gives 0.",
    syntax_example = "ST_SRID(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_setsrid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub struct SRID;

impl SRID {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SRID {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SRID {
    fn name(&self) -> &str {
        "st_srid"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(srid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn srid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let srid = crs_to_srid(input_metadata(&args.arg_fields[0]).crs()).unwrap_or(SRID_UNKNOWN);
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &SridKernel { srid })?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct SridKernel {
    srid: i32,
}

impl GeometryKernel for SridKernel {
    type Output = i32;

    fn eval(
        &self,
        _geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        Ok(Some(self.srid))
    }
}
