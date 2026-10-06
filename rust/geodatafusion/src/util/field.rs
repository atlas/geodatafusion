//! Geometry arguments, their metadata, and the fields of geometry results.

use std::sync::Arc;

use arrow_array::BinaryArray;
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::internal_datafusion_err;
use datafusion::logical_expr::ScalarFunctionArgs;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{WkbArray, from_arrow_array};
use geoarrow_schema::{Metadata, WkbType};

use crate::error::GeoDataFusionResult;

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

/// The field of a WKB geometry result with the given metadata, named after the UDF.
pub(crate) fn wkb_return_field(name: &str, metadata: Arc<Metadata>) -> FieldRef {
    Arc::new(Field::new(name, DataType::Binary, true).with_extension_type(WkbType::new(metadata)))
}
