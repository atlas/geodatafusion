use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::array::{LargeWkbArray, WkbArray, WkbViewArray};
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::GeoArrowType;

use crate::error::GeoDataFusionResult;
use crate::util::field::{input_metadata, wkb_return_field};

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
