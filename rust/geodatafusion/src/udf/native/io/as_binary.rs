//! Well-Known Binary output: ST_AsBinary, ST_AsEWKB and ST_AsHEXEWKB.

use std::fmt::Write;
use std::sync::{Arc, LazyLock};

use arrow_array::{Array, BinaryArray, StringArray};
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::wkb::{parse_endianness, write_ewkb, write_wkb};
use crate::util::args::optional_text_arg;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// PostGIS: ST_AsBinary(geometry) and ST_AsBinary(geometry, text NDRorXDR), and the same for
/// ST_AsEWKB and ST_AsHEXEWKB. PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Text]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Returns the ISO Well-Known Binary of a geometry.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the OGC/ISO Well-Known Binary (WKB) representation of the geometry, without SRID. The byte order is little-endian (NDR) unless NDRorXDR is 'XDR', which selects big-endian.",
    syntax_example = "ST_AsBinary(g1, NDRorXDR)",
    argument(name = "g1", description = "geometry"),
    argument(name = "NDRorXDR", description = "text"),
    related_udf(name = "st_asewkb"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsBinary;

impl AsBinary {
    pub fn new() -> Self {
        Self
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
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Binary)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_binary_impl(args, Flavor::Iso, Encoding::Binary)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the Extended Well-Known Binary of a geometry.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the Extended Well-Known Binary (EWKB) representation of the geometry, with SRID. The byte order is little-endian (NDR) unless NDRorXDR is 'XDR', which selects big-endian. A CRS that doesn't name an SRID is written without one.",
    syntax_example = "ST_AsEWKB(g1, NDRorXDR)",
    argument(name = "g1", description = "geometry"),
    argument(name = "NDRorXDR", description = "text"),
    related_udf(name = "st_asbinary"),
    related_udf(name = "st_ashexewkb"),
    related_udf(name = "st_geomfromewkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsEWKB;

impl AsEWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsEWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsEWKB {
    fn name(&self) -> &str {
        "st_asewkb"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Binary)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_binary_impl(args, Flavor::Extended, Encoding::Binary)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the Extended Well-Known Binary of a geometry as hex text.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the Extended Well-Known Binary (EWKB) representation of the geometry as uppercase hex text. The byte order is little-endian (NDR) unless NDRorXDR is 'XDR', which selects big-endian. A CRS that doesn't name an SRID is written without one.",
    syntax_example = "ST_AsHEXEWKB(g1, NDRorXDR)",
    argument(name = "g1", description = "geometry"),
    argument(name = "NDRorXDR", description = "text"),
    related_udf(name = "st_asewkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsHEXEWKB;

impl AsHEXEWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsHEXEWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsHEXEWKB {
    fn name(&self) -> &str {
        "st_ashexewkb"
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
        Ok(as_binary_impl(args, Flavor::Extended, Encoding::Hex)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum Flavor {
    Iso,
    Extended,
}

#[derive(Debug, Clone, Copy)]
enum Encoding {
    Binary,
    Hex,
}

fn as_binary_impl(
    args: ScalarFunctionArgs,
    flavor: Flavor,
    encoding: Encoding,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsBinaryKernel {
        flavor,
        // EWKB carries the column's SRID; one that isn't an SRID is left out.
        srid: crs_to_srid(input_metadata(&args.arg_fields[0]).crs()).unwrap_or(SRID_UNKNOWN),
        endianness: optional_text_arg(&args, 1, "NDR")?,
    };
    let wkb: Vec<Option<Vec<u8>>> = map_geometry(geometries.as_ref(), &kernel)?;
    let result: Arc<dyn Array> = match encoding {
        Encoding::Binary => Arc::new(BinaryArray::from_iter(wkb)),
        Encoding::Hex => Arc::new(StringArray::from_iter(
            wkb.iter().map(|wkb| wkb.as_deref().map(hex)),
        )),
    };
    Ok(ColumnarValue::Array(result))
}

struct AsBinaryKernel {
    flavor: Flavor,
    srid: i32,
    endianness: StringArray,
}

impl GeometryKernel for AsBinaryKernel {
    type Output = Vec<u8>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Vec<u8>>> {
        // SQL NULL in, SQL NULL out.
        if self.endianness.is_null(row) {
            return Ok(None);
        }
        let endianness = parse_endianness(self.endianness.value(row));
        let mut out = vec![];
        match self.flavor {
            Flavor::Iso => write_wkb(&mut out, geom, endianness)?,
            Flavor::Extended => write_ewkb(&mut out, geom, self.srid, endianness),
        }
        Ok(Some(out))
    }
}

/// Uppercase hex, as PostGIS writes it.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 * bytes.len());
    for byte in bytes {
        write!(out, "{byte:02X}").expect("writing to a String doesn't fail");
    }
    out
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use arrow_array::RecordBatch;
    use arrow_schema::Schema;
    use datafusion::prelude::SessionContext;
    use geoarrow_array::GeoArrowArray;
    use geoarrow_array::array::WkbArray;
    use geoarrow_array::cast::to_wkb;
    use geoarrow_array::test::point;
    use geoarrow_schema::{CoordType, Crs, Dimension, Metadata};

    use super::*;
    use crate::udf::native::io::GeomFromWKB;

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

        // PostGIS returns bytea, so the CRS isn't kept.
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        assert_eq!(output_field.data_type(), &DataType::Binary);
        assert_eq!(output_field.extension_type_name(), None);

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

        // The CRS doesn't survive ST_AsBinary, as in PostGIS.
        assert_eq!(wkb_arr.inner(), to_wkb::<i32>(&point_arr).unwrap().inner());
    }
}
