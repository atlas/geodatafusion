//! Readers for non-geometry UDF arguments.

use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, Int32Type};
use arrow_array::{BooleanArray, Float64Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::{ScalarValue, internal_err, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::{ReturnFieldArgs, ScalarFunctionArgs};

use crate::util::srid::clamp_srid;

/// Optional `integer` argument `index`, one value per row (constants are broadcast), or `default`
/// in every row if the call doesn't have it.
pub(crate) fn optional_int_arg(
    args: &ScalarFunctionArgs,
    index: usize,
    default: i32,
) -> Result<Int32Array> {
    let Some(arg) = args.args.get(index) else {
        return Ok(Int32Array::from_value(default, args.number_rows));
    };
    let array = arg
        .cast_to(&DataType::Int32, None)?
        .to_array(args.number_rows)?;
    Ok(array.as_primitive::<Int32Type>().clone())
}

/// Optional `boolean` argument `index`, one value per row (constants are broadcast), or
/// `default` in every row if the call doesn't have it.
#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(dead_code, reason = "only GEOS-backed UDFs take a boolean so far")
)]
pub(crate) fn optional_bool_arg(
    args: &ScalarFunctionArgs,
    index: usize,
    default: bool,
) -> Result<BooleanArray> {
    let Some(arg) = args.args.get(index) else {
        return Ok(BooleanArray::from(vec![default; args.number_rows]));
    };
    let array = arg
        .cast_to(&DataType::Boolean, None)?
        .to_array(args.number_rows)?;
    Ok(array.as_boolean().clone())
}

/// Optional `text` argument `index`, one value per row (constants are broadcast), or `default`
/// in every row if the call doesn't have it.
pub(crate) fn optional_text_arg(
    args: &ScalarFunctionArgs,
    index: usize,
    default: &str,
) -> Result<StringArray> {
    let Some(arg) = args.args.get(index) else {
        return Ok(StringArray::from_iter_values(std::iter::repeat_n(
            default,
            args.number_rows,
        )));
    };
    let array = arg
        .cast_to(&DataType::Utf8, None)?
        .to_array(args.number_rows)?;
    Ok(array.as_string::<i32>().clone())
}

/// Optional `float8` argument `index`, one value per row (constants are broadcast), or `default`
/// in every row if the call doesn't have it.
pub(crate) fn optional_float_arg(
    args: &ScalarFunctionArgs,
    index: usize,
    default: f64,
) -> Result<Float64Array> {
    let Some(arg) = args.args.get(index) else {
        return Ok(Float64Array::from_value(default, args.number_rows));
    };
    let array = arg
        .cast_to(&DataType::Float64, None)?
        .to_array(args.number_rows)?;
    Ok(array.as_primitive::<Float64Type>().clone())
}

/// The constant SRID argument `index`, read when planning and clamped like PostGIS. `None` if
/// the SRID is NULL, in which case the function returns NULL.
///
/// The SRID becomes the output column's CRS, so it must be constant: anything else is a plan
/// error. Only call this when the call has the argument.
pub(crate) fn scalar_srid(name: &str, args: &ReturnFieldArgs, index: usize) -> Result<Option<i32>> {
    match args.scalar_arguments.get(index) {
        None => internal_err!("{name}: argument {index} doesn't exist"),
        Some(None) => plan_err!("{name} only supports a constant SRID"),
        Some(Some(value)) if value.is_null() => Ok(None),
        Some(Some(value)) => match value.cast_to(&DataType::Int64)? {
            ScalarValue::Int64(Some(srid)) => Ok(Some(clamp_srid(srid))),
            other => internal_err!("{name}: SRID cast to Int64 gave {other:?}"),
        },
    }
}
