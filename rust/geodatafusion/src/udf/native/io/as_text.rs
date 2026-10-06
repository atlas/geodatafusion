use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_schema::WktType;

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::number::DEFAULT_MAX_DECIMAL_DIGITS;
use crate::udf::native::io::util::wkt::write_wkt;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AsText(geometry g1, integer maxdecimaldigits = 15).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "maxdecimaldigits"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the Well-Known Text (WKT) representation of the geometry.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the OGC Well-Known Text (WKT) representation of the geometry. Coordinates are written with at most maxdecimaldigits decimals (default 15), rounded half to even, as PostGIS writes them.",
    syntax_example = "ST_AsText(g1, maxdecimaldigits)",
    argument(name = "g1", description = "geometry"),
    argument(name = "maxdecimaldigits", description = "integer"),
    related_udf(name = "st_asbinary"),
    related_udf(name = "st_geomfromtext")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsText;

impl AsText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsText {
    fn name(&self) -> &str {
        "st_astext"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        // TODO: return plain Utf8 with the other breaking output changes (plans/README.md, D3).
        let wkt_type = WktType::new(input_metadata(&args.arg_fields[0]));
        Ok(Arc::new(
            Field::new(self.name(), DataType::Utf8, true).with_extension_type(wkt_type),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_text_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_text_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsTextKernel {
        max_decimal_digits: optional_int_arg(&args, 1, DEFAULT_MAX_DECIMAL_DIGITS)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsTextKernel {
    max_decimal_digits: Int32Array,
}

impl GeometryKernel for AsTextKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // SQL NULL in, SQL NULL out.
        if self.max_decimal_digits.is_null(row) {
            return Ok(None);
        }
        let mut wkt = String::new();
        write_wkt(&mut wkt, geom, self.max_decimal_digits.value(row));
        Ok(Some(wkt))
    }
}

#[cfg(test)]
mod test {
    use arrow_array::RecordBatch;
    use arrow_schema::Schema;
    use datafusion::prelude::SessionContext;
    use geoarrow_array::GeoArrowArray;
    use geoarrow_array::test::point;
    use geoarrow_schema::crs::Crs;
    use geoarrow_schema::{CoordType, Dimension, Metadata};

    use super::*;

    #[tokio::test]
    async fn test_as_text() {
        let ctx = SessionContext::new();

        let crs = Crs::from_authority_code("EPSG:4326".to_string());
        let metadata = Arc::new(Metadata::new(crs.clone(), Default::default()));

        let geo_arr = point::array(CoordType::Separated, Dimension::XY).with_metadata(metadata);

        let arr = geo_arr.to_array_ref();
        let field = geo_arr.data_type().to_field("geometry", true);
        let schema = Schema::new([Arc::new(field)]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![arr]).unwrap();

        ctx.register_batch("t", batch).unwrap();

        ctx.register_udf(AsText::new().into());

        let sql_df = ctx.sql("SELECT ST_AsText(geometry) FROM t;").await.unwrap();

        let output_batches = sql_df.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];

        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        let output_wkb_type = output_field.try_extension_type::<WktType>().unwrap();

        assert_eq!(&crs, output_wkb_type.metadata().crs());
    }
}
