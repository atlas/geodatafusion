use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geoarrow_schema::CoordType;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_result, wkb_return_field};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a point which is the geometric center of mass of a geometry.",
    syntax_example = "ST_Centroid(geometry)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Centroid;

impl Centroid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Centroid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Centroid {
    fn name(&self) -> &str {
        "st_centroid"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
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

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(centroid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn centroid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    // geoarrow-expr-geo builds a native array; the result is converted to WKB.
    let result = geoarrow_expr_geo::centroid(&geometries, CoordType::default())?;
    wkb_result(&result, &args.return_field)
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_centroid_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(Centroid.into());
        ctx.register_udf(GeomFromText::default().into());

        let sql = "SELECT ST_Centroid(ST_GeomFromText('MULTIPOINT(0 0,2 2)', 4326))";
        assert_wkb_output(&ctx, sql, 4326).await;
    }
}
