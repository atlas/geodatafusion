use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{LargeWktArray, WktArray, WktViewArray};
use geoarrow_array::cast::from_wkt;
use geoarrow_schema::{CoordType, GeoArrowType, GeometryType, Metadata};

use crate::error::GeoDataFusionResult;

#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a geometry object from the OGC Well-Known text representation.",
    syntax_example = "ST_GeomFromText(text)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromText {
    signature: Signature,
    coord_type: CoordType,
    aliases: Vec<String>,
}

impl GeomFromText {
    pub fn new(coord_type: CoordType) -> Self {
        Self {
            signature: Signature::uniform(
                1,
                vec![DataType::Utf8, DataType::LargeUtf8, DataType::Utf8View],
                Volatility::Immutable,
            ),
            coord_type,
            aliases: vec!["st_geometryfromtext".to_string(), "st_wkttosql".to_string()],
        }
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
        let array = &ColumnarValue::values_to_arrays(&args.args)?[0];
        let field = &args.arg_fields[0];
        let to_type = GeoArrowType::from_arrow_field(args.return_field.as_ref())?;
        let geom_arr = match field.data_type() {
            DataType::Utf8 => from_wkt(
                &WktArray::try_from((array.as_ref(), field.as_ref()))?,
                to_type,
            ),
            DataType::LargeUtf8 => from_wkt(
                &LargeWktArray::try_from((array.as_ref(), field.as_ref()))?,
                to_type,
            ),
            DataType::Utf8View => from_wkt(
                &WktViewArray::try_from((array.as_ref(), field.as_ref()))?,
                to_type,
            ),
            _ => unreachable!(),
        }?;

        Ok(ColumnarValue::Array(geom_arr.to_array_ref()))
    }
}

impl Default for GeomFromText {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl ScalarUDFImpl for GeomFromText {
    fn name(&self) -> &str {
        "st_geomfromtext"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<Arc<Field>> {
        let input_field = &args.arg_fields[0];
        let metadata = Arc::new(Metadata::try_from(input_field.as_ref())?);
        let geom_type = GeometryType::new(metadata).with_coord_type(self.coord_type);
        Ok(geom_type
            .to_field(input_field.name(), input_field.is_nullable())
            .into())
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(self.invoke_with_args(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;
    use geoarrow_schema::CoordType;

    use super::*;

    #[tokio::test]
    async fn test_from_text() {
        let ctx = SessionContext::new();

        ctx.register_udf(GeomFromText::new(CoordType::Separated).into());

        let sql_df = ctx
            .sql(r#"SELECT ST_GeomFromText('POINT(30 10)');"#)
            .await
            .unwrap();

        sql_df.show().await.unwrap();
    }
}
