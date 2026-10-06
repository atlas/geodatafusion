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
    description = "Tests if an ST_Geometry value is well-formed and valid in 2D according to the OGC rules",
    syntax_example = "ST_IsValid(geomA)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValid;

impl IsValid {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for IsValid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValid {
    fn name(&self) -> &str {
        "st_isvalid"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
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
    let result = geoarrow_expr_geo::validation::is_valid(&geo_array)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}
