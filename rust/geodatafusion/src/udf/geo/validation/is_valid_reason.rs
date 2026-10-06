use std::sync::Arc;

use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geoarrow_array::array::from_arrow_array;

use crate::error::GeoDataFusionResult;
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Validation"),
    description = "Returns text stating if a geometry is valid, or if invalid a reason why.",
    syntax_example = "ST_IsValidReason(geomA)",
    argument(name = "geomA", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValidReason;

impl IsValidReason {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for IsValidReason {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValidReason {
    fn name(&self) -> &str {
        "st_isvalidreason"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_valid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_valid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let arrays = ColumnarValue::values_to_arrays(&args.args)?;
    let geo_array = from_arrow_array(&arrays[0], &args.arg_fields[0])?;
    // geoarrow-expr-geo returns Utf8View; PostGIS returns text.
    let result = geoarrow_expr_geo::validation::is_valid_reason(&geo_array)?;
    Ok(ColumnarValue::Array(Arc::new(result)).cast_to(&DataType::Utf8, None)?)
}
