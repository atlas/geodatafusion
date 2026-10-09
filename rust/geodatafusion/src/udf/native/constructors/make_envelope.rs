use std::sync::{Arc, LazyLock};

use arrow_array::{Array, new_null_array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{Crs, GeoArrowType, Metadata};
use wkt::types::{Coord, Dimension, LineString, Polygon};

use crate::error::GeoDataFusionResult;
use crate::util::args::{optional_float_arg, scalar_srid};
use crate::util::field::wkb_return_field;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_MakeEnvelope(float xmin, float ymin, float xmax, float ymax, integer srid=unknown).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Float, Arg::Srid],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["xmin", "ymin", "xmax", "ymax", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a rectangular Polygon from minimum and maximum coordinates.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a rectangular POLYGON from minimum and maximum coordinates, with the corners (xmin ymin, xmin ymax, xmax ymax, xmax ymin). The coordinates are used as given, so a degenerate or inverted rectangle keeps all five points. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_MakeEnvelope(xmin, ymin, xmax, ymax, srid)",
    argument(name = "xmin", description = "float8"),
    argument(name = "ymin", description = "float8"),
    argument(name = "xmax", description = "float8"),
    argument(name = "ymax", description = "float8"),
    argument(name = "srid", description = "integer, default unknown"),
    related_udf(name = "st_makebox2d"),
    related_udf(name = "st_tileenvelope")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeEnvelope;

impl MakeEnvelope {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakeEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakeEnvelope {
    fn name(&self) -> &str {
        "st_makeenvelope"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        make_envelope_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(make_envelope_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_envelope_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let crs = if args.arg_fields.len() > 4 {
        scalar_srid(name, args, 4)?
            .map(srid_to_crs)
            .unwrap_or_default()
    } else {
        Crs::default()
    };
    Ok(wkb_return_field(name, Arc::new(Metadata::new(crs, None))))
}

fn make_envelope_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    // A NULL SRID gives NULL in every row.
    if matches!(args.args.get(4), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let bounds = [
        optional_float_arg(&args, 0, 0.0)?,
        optional_float_arg(&args, 1, 0.0)?,
        optional_float_arg(&args, 2, 0.0)?,
        optional_float_arg(&args, 3, 0.0)?,
    ];
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(
            internal_datafusion_err!("st_makeenvelope: expected a WKB return field").into(),
        );
    };
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for row in 0..args.number_rows {
        // SQL NULL in, SQL NULL out.
        if bounds.iter().any(|array| array.is_null(row)) {
            builder.push_geometry(None::<&Polygon<f64>>)?;
            continue;
        }
        let [xmin, ymin, xmax, ymax] = bounds.each_ref().map(|array| array.value(row));
        let corner = |x, y| Coord {
            x,
            y,
            z: None,
            m: None,
        };
        let ring = vec![
            corner(xmin, ymin),
            corner(xmin, ymax),
            corner(xmax, ymax),
            corner(xmax, ymin),
            corner(xmin, ymin),
        ];
        let polygon = Polygon::new(vec![LineString::new(ring, Dimension::XY)], Dimension::XY);
        builder.push_geometry(Some(&polygon))?;
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}
