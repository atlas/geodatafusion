use std::sync::{Arc, LazyLock};

use arrow_array::Array;
use arrow_array::cast::AsArray;
use arrow_schema::{DataType, FieldRef};
use datafusion::arrow::compute::cast;
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, Metadata};
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::polyline::{DEFAULT_PRECISION, decode};
use crate::util::args::optional_int_arg;
use crate::util::field::wkb_return_field;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_LineFromEncodedPolyline(text txtin, integer nprecision = 5).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text], &[Arg::Text, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["txtin", "nprecision"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a LineString from an Encoded Polyline.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the LINESTRING an Encoded Polyline describes, with SRID 4326. nprecision is the number of decimal digits it was encoded with (default 5; a negative value means the default). An empty string gives LINESTRING EMPTY; as in PostGIS, the text isn't validated.",
    syntax_example = "ST_LineFromEncodedPolyline(txtin, nprecision)",
    argument(name = "txtin", description = "text"),
    argument(name = "nprecision", description = "integer, default 5"),
    related_udf(name = "st_asencodedpolyline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineFromEncodedPolyline;

impl LineFromEncodedPolyline {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineFromEncodedPolyline {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineFromEncodedPolyline {
    fn name(&self) -> &str {
        "st_linefromencodedpolyline"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            Arc::new(Metadata::new(srid_to_crs(4326), None)),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(line_from_encoded_polyline_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_from_encoded_polyline_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(exec_datafusion_err!("{name}: expected a WKB return field").into());
    };
    let text = cast(&args.args[0].to_array(args.number_rows)?, &DataType::Utf8)?;
    let precision = optional_int_arg(&args, 1, DEFAULT_PRECISION)?;
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for (row, value) in text.as_string::<i32>().iter().enumerate() {
        // SQL NULL in any argument, SQL NULL out.
        let Some(value) = value.filter(|_| !precision.is_null(row)) else {
            builder.push_geometry(None::<&LineString<f64>>)?;
            continue;
        };
        let coords = decode(value, precision.value(row))
            .into_iter()
            .map(|(x, y)| Coord {
                x,
                y,
                z: None,
                m: None,
            })
            .collect();
        builder.push_geometry(Some(&LineString::new(coords, Dimension::XY)))?;
    }
    Ok(ColumnarValue::Array(builder.finish().to_array_ref()))
}
