use std::sync::LazyLock;

use arrow_array::{Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_coords;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_QuantizeCoordinates(geometry g, int prec_x, int prec_y = NULL,
/// int prec_z = NULL, int prec_m = NULL).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer, Arg::Integer],
    &[
        Arg::Geometry,
        Arg::Integer,
        Arg::Integer,
        Arg::Integer,
        Arg::Integer,
    ],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g", "prec_x", "prec_y", "prec_z", "prec_m"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Sets least significant bits of coordinates to zero.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Zeroes the mantissa bits of each ordinate that aren't needed to represent it with the given number of digits after the decimal point, which makes the coordinates compress better while they still round to the same values. prec_y, prec_z and prec_m default to prec_x, also when NULL; a NULL prec_x is an error, as in PostGIS.",
    syntax_example = "ST_QuantizeCoordinates(g, prec_x, prec_y, prec_z, prec_m)",
    argument(name = "g", description = "geometry"),
    argument(
        name = "prec_x",
        description = "integer, digits after the decimal point"
    ),
    argument(name = "prec_y", description = "integer, default prec_x"),
    argument(name = "prec_z", description = "integer, default prec_x"),
    argument(name = "prec_m", description = "integer, default prec_x"),
    related_udf(name = "st_snaptogrid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct QuantizeCoordinates;

impl QuantizeCoordinates {
    pub fn new() -> Self {
        Self
    }
}

impl Default for QuantizeCoordinates {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for QuantizeCoordinates {
    fn name(&self) -> &str {
        "st_quantizecoordinates"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
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

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(quantize_coordinates_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn quantize_coordinates_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    // A missing precision is NULL, which means prec_x.
    let precision = |index| match args.args.get(index) {
        Some(_) => optional_int_arg(&args, index, 0),
        None => Ok(Int32Array::new_null(args.number_rows)),
    };
    let kernel = QuantizeCoordinatesKernel {
        precisions: [precision(1)?, precision(2)?, precision(3)?, precision(4)?],
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// The digits for X, Y, Z and M.
struct QuantizeCoordinatesKernel {
    precisions: [Int32Array; 4],
}

impl GeometryKernel for QuantizeCoordinatesKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let [prec_x, prec_y, prec_z, prec_m] = &self.precisions;
        if prec_x.is_null(row) {
            return Err(
                exec_datafusion_err!("st_quantizecoordinates: Must specify precision").into(),
            );
        }
        let x = prec_x.value(row);
        let digits = |array: &Int32Array| {
            if array.is_null(row) {
                x
            } else {
                array.value(row)
            }
        };
        let (y, z, m) = (digits(prec_y), digits(prec_z), digits(prec_m));
        Ok(Some(map_coords(geom, &|c| Coord {
            x: quantize(c.x, x),
            y: quantize(c.y, y),
            z: c.z.map(|value| quantize(value, z)),
            m: c.m.map(|value| quantize(value, m)),
        })))
    }
}

/// Zeroes the mantissa bits of `value` below those needed for `digits` decimal digits.
///
/// The bits kept are the value's binary exponent plus one, plus ceil(digits * log2(10)) for the
/// fraction, and always at least one. This is the rule the PostGIS docs describe, fitted to
/// PostGIS's results: it matched all 8954 of a sample of values from 2^-30 to 2^40 with -5 to 16
/// digits.
fn quantize(value: f64, digits: i32) -> f64 {
    const MANTISSA_BITS: i64 = 52;
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let bits = value.to_bits();
    let exponent = ((bits >> MANTISSA_BITS) & 0x7ff) as i64 - 1023;
    let fraction_bits = (f64::from(digits) * std::f64::consts::LOG2_10).ceil() as i64;
    let keep = (exponent + 1 + fraction_bits).max(1);
    if keep >= MANTISSA_BITS {
        return value;
    }
    let mask = !((1u64 << (MANTISSA_BITS - keep)) - 1);
    f64::from_bits(bits & mask)
}
