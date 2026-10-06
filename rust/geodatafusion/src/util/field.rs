//! Geometry arguments, their metadata, and the fields of geometry results.

use std::sync::Arc;

use arrow_array::{Array, BinaryArray};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::{exec_err, internal_datafusion_err};
use datafusion::error::Result;
use datafusion::logical_expr::{ColumnarValue, ScalarFunctionArgs};
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{WkbArray, from_arrow_array};
use geoarrow_array::cast::to_wkb;
use geoarrow_schema::{Metadata, WkbType};

use crate::error::GeoDataFusionResult;
use crate::util::srid::crs_to_srid;

/// Decodes geometry argument `index`, whatever its GeoArrow encoding. Untagged `Binary` is read
/// as WKB and untagged strings as WKT. A NULL literal gives an all-NULL WKB array.
pub(crate) fn geometry_array(
    args: &ScalarFunctionArgs,
    index: usize,
) -> GeoDataFusionResult<Arc<dyn GeoArrowArray>> {
    let (Some(arg), Some(field)) = (args.args.get(index), args.arg_fields.get(index)) else {
        return Err(internal_datafusion_err!("argument {index} doesn't exist").into());
    };
    let array = arg.to_array(args.number_rows)?;
    if array.data_type() == &DataType::Null {
        let nulls = BinaryArray::new_null(array.len());
        return Ok(Arc::new(WkbArray::new(nulls, Default::default())));
    }
    Ok(from_arrow_array(&array, field)?)
}

/// The GeoArrow metadata (CRS and edges) of a field; default for untagged fields.
pub(crate) fn input_metadata(field: &Field) -> Arc<Metadata> {
    Arc::new(Metadata::try_from(field).unwrap_or_default())
}

/// The metadata shared by the geometry arguments at `indices`. Different SRIDs are an error, as
/// in PostGIS ("Operation on mixed SRID geometries").
///
/// Call it at the start of the `_impl` function of a UDF with several geometry arguments. An
/// argument that is NULL in every row is skipped: PostGIS functions are STRICT and return NULL
/// before comparing SRIDs.
pub(crate) fn common_metadata(
    name: &str,
    args: &ScalarFunctionArgs,
    indices: &[usize],
) -> Result<Arc<Metadata>> {
    let mut common: Option<(Arc<Metadata>, Option<i32>)> = None;
    for &index in indices {
        let (Some(arg), Some(field)) = (args.args.get(index), args.arg_fields.get(index)) else {
            return Err(internal_datafusion_err!(
                "{name}: argument {index} doesn't exist"
            ));
        };
        let all_null = match arg {
            ColumnarValue::Scalar(scalar) => scalar.is_null(),
            ColumnarValue::Array(array) => array.null_count() == array.len(),
        };
        if all_null {
            continue;
        }
        let metadata = input_metadata(field);
        let srid = crs_to_srid(metadata.crs());
        match &common {
            None => common = Some((metadata, srid)),
            Some((common_metadata, common_srid)) => {
                let same = match (common_srid, srid) {
                    (Some(a), Some(b)) => *a == b,
                    _ => common_metadata.crs() == metadata.crs(),
                };
                if !same {
                    return exec_err!(
                        "{name}: Operation on mixed SRID geometries ({} != {})",
                        describe_srid(*common_srid),
                        describe_srid(srid)
                    );
                }
            }
        }
    }
    Ok(common.map(|(metadata, _)| metadata).unwrap_or_default())
}

fn describe_srid(srid: Option<i32>) -> String {
    srid.map_or_else(|| "a CRS without SRID".to_string(), |srid| srid.to_string())
}

/// The field of a WKB geometry result with the given metadata, named after the UDF.
pub(crate) fn wkb_return_field(name: &str, metadata: Arc<Metadata>) -> FieldRef {
    Arc::new(Field::new(name, DataType::Binary, true).with_extension_type(WkbType::new(metadata)))
}

/// A geometry result as WKB, with the metadata of the UDF's return field.
///
/// For UDFs whose algorithm still produces a native GeoArrow array; the result is converted once.
pub(crate) fn wkb_result(
    array: &dyn GeoArrowArray,
    return_field: &Field,
) -> GeoDataFusionResult<ColumnarValue> {
    let wkb = to_wkb::<i32>(array)?;
    let result = WkbArray::new(wkb.inner().clone(), input_metadata(return_field));
    Ok(ColumnarValue::Array(result.into_array_ref()))
}
