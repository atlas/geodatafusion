//! Well-Known Text output: ST_AsText and ST_AsEWKT.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::number::DEFAULT_MAX_DECIMAL_DIGITS;
use crate::udf::native::io::util::wkt::{WktFlavor, write_wkt};
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// PostGIS: ST_AsText(geometry g1, integer maxdecimaldigits = 15), and the same for ST_AsEWKT.
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
        Ok(DataType::Utf8)
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
        write_wkt(
            &mut wkt,
            geom,
            WktFlavor::Iso,
            self.max_decimal_digits.value(row),
        );
        Ok(Some(wkt))
    }
}

static EWKT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "maxdecimaldigits"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the Well-Known Text (WKT) representation of the geometry with SRID metadata.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the Extended Well-Known Text (EWKT) representation of the geometry: WKT prefixed with SRID=n; when the SRID isn't 0. Coordinates are written with at most maxdecimaldigits decimals (default 15), as PostGIS writes them. A CRS that doesn't name an SRID is written without a prefix.",
    syntax_example = "ST_AsEWKT(g1, maxdecimaldigits)",
    argument(name = "g1", description = "geometry"),
    argument(name = "maxdecimaldigits", description = "integer"),
    related_udf(name = "st_astext"),
    related_udf(name = "st_geomfromewkt")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsEWKT;

impl AsEWKT {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsEWKT {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsEWKT {
    fn name(&self) -> &str {
        "st_asewkt"
    }

    fn signature(&self) -> &Signature {
        &EWKT_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_ewkt_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_ewkt_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    // The SRID is the column's, so the prefix is the same for every row.
    let prefix = match crs_to_srid(input_metadata(&args.arg_fields[0]).crs()) {
        Some(srid) if srid != SRID_UNKNOWN => format!("SRID={srid};"),
        _ => String::new(),
    };
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsEWKTKernel {
        prefix,
        max_decimal_digits: optional_int_arg(&args, 1, DEFAULT_MAX_DECIMAL_DIGITS)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsEWKTKernel {
    prefix: String,
    max_decimal_digits: Int32Array,
}

impl GeometryKernel for AsEWKTKernel {
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
        let mut ewkt = self.prefix.clone();
        write_wkt(
            &mut ewkt,
            geom,
            WktFlavor::Extended,
            self.max_decimal_digits.value(row),
        );
        Ok(Some(ewkt))
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

        // PostGIS returns text, so the CRS isn't kept.
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        assert_eq!(output_field.data_type(), &DataType::Utf8);
        assert_eq!(output_field.extension_type_name(), None);
    }
}
