use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::array::{LargeWkbArray, WkbArray, WkbViewArray, from_arrow_array};
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::cast::to_wkb;
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::{GeoArrowType, Metadata, WkbType};

use crate::error::{GeoDataFusionError, GeoDataFusionResult};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the OGC/ISO Well-Known Binary (WKB) representation of the geometry.",
    syntax_example = "ST_AsBinary(geometry)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsBinary;

impl AsBinary {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AsBinary {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsBinary {
    fn name(&self) -> &str {
        "st_asbinary"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<Arc<Field>> {
        let input_field = &args.arg_fields[0];
        let metadata = Arc::new(Metadata::try_from(input_field.as_ref()).unwrap_or_default());
        let wkb_type = WkbType::new(metadata);
        Ok(Field::new(
            input_field.name(),
            DataType::Binary,
            input_field.is_nullable(),
        )
        .with_extension_type(wkb_type)
        .into())
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let array = &ColumnarValue::values_to_arrays(&args.args)?[0];
        let field = &args.arg_fields[0];
        let geo_array = from_arrow_array(&array, field).map_err(GeoDataFusionError::GeoArrow)?;
        let wkb_arr = to_wkb::<i32>(geo_array.as_ref()).map_err(GeoDataFusionError::GeoArrow)?;
        Ok(ColumnarValue::Array(wkb_arr.into_array_ref()))
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Takes a well-known binary representation of a geometry and a Spatial Reference System ID (SRID) and creates an instance of the appropriate geometry type",
    syntax_example = "ST_GeomFromWKB(buffer)",
    argument(name = "geom", description = "bytea")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromWKB {
    signature: Signature,
    aliases: Vec<String>,
}

impl GeomFromWKB {
    pub fn new() -> Self {
        Self {
            signature: Signature::uniform(
                1,
                vec![
                    DataType::Binary,
                    DataType::LargeBinary,
                    DataType::BinaryView,
                ],
                Volatility::Immutable,
            ),
            aliases: vec!["st_wkbtosql".to_string()],
        }
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
        let array = &ColumnarValue::values_to_arrays(&args.args)?[0];
        let field = &args.arg_fields[0];
        let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(&args.return_field)?
        else {
            return Err(internal_datafusion_err!("st_geomfromwkb: unexpected return field").into());
        };
        // Parsing validates the input, and writing gives little-endian ISO WKB whatever the input
        // byte order or flavor.
        let mut builder = WkbBuilder::<i32>::new(output_type);
        match field.data_type() {
            DataType::Binary => rewrite_wkb(
                &WkbArray::try_from((array.as_ref(), field.as_ref()))?,
                &mut builder,
            ),
            DataType::LargeBinary => rewrite_wkb(
                &LargeWkbArray::try_from((array.as_ref(), field.as_ref()))?,
                &mut builder,
            ),
            DataType::BinaryView => rewrite_wkb(
                &WkbViewArray::try_from((array.as_ref(), field.as_ref()))?,
                &mut builder,
            ),
            data_type => {
                return Err(internal_datafusion_err!(
                    "st_geomfromwkb: unexpected argument type {data_type}"
                )
                .into());
            }
        }?;
        Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
    }
}

fn rewrite_wkb<'a>(
    array: &'a impl GeoArrowArrayAccessor<'a>,
    builder: &mut WkbBuilder<i32>,
) -> GeoDataFusionResult<()> {
    for geometry in array.iter() {
        builder.push_geometry(geometry.transpose()?.as_ref())?;
    }
    Ok(())
}

impl Default for GeomFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromWKB {
    fn name(&self) -> &str {
        "st_geomfromwkb"
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
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
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
    use std::sync::Arc;

    use arrow_array::RecordBatch;
    use arrow_schema::Schema;
    use datafusion::prelude::SessionContext;
    use geoarrow_array::test::point;
    use geoarrow_schema::{CoordType, Crs, Dimension, Metadata};

    use super::*;

    #[tokio::test]
    async fn test_as_binary() {
        let ctx = SessionContext::new();

        let crs = Crs::from_authority_code("EPSG:4326".to_string());
        let metadata = Arc::new(Metadata::new(crs.clone(), Default::default()));

        let point_arr = point::array(CoordType::Separated, Dimension::XY).with_metadata(metadata);

        let arr = point_arr.to_array_ref();
        let field = point_arr.data_type().to_field("geometry", true);
        let schema = Schema::new([Arc::new(field)]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![arr]).unwrap();

        ctx.register_batch("t", batch).unwrap();

        ctx.register_udf(AsBinary::new().into());
        ctx.register_udf(GeomFromWKB::new().into());

        let sql_df = ctx
            .sql("SELECT ST_AsBinary(geometry) FROM t;")
            .await
            .unwrap();

        let output_batches = sql_df.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];

        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        let output_wkb_type = output_field.try_extension_type::<WkbType>().unwrap();

        assert_eq!(&crs, output_wkb_type.metadata().crs());

        let sql_df2 = ctx
            .sql("SELECT ST_GeomFromWKB(ST_AsBinary(geometry)) FROM t;")
            .await
            .unwrap();

        let output_batches = sql_df2.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        let output_column = output_batch.column(0);
        let wkb_arr = WkbArray::try_from((output_column.as_ref(), output_field)).unwrap();

        assert_eq!(wkb_arr, to_wkb::<i32>(&point_arr).unwrap());
    }
}
