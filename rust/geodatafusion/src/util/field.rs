//! Geometry arguments, their metadata, and the fields of geometry results.

use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef, BinaryArray};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::arrow::compute::cast;
use datafusion::common::{exec_datafusion_err, exec_err, internal_datafusion_err};
use datafusion::error::Result;
use datafusion::logical_expr::{ColumnarValue, ScalarFunctionArgs};
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{WkbArray, from_arrow_array};
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{Metadata, WkbType};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::ewkt::parse_ewkt;
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// Decodes geometry argument `index`, whatever its GeoArrow encoding. Untagged `Binary` is read
/// as WKB and untagged strings as PostGIS (E)WKT (see [`geometries_from_array`]). A NULL literal
/// gives an all-NULL WKB array.
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
    geometries_from_array(&array, field)
}

/// Decodes an array of geometries with the type of `field`.
///
/// Untagged strings are text that PostgreSQL would cast to `geometry` implicitly, so they are
/// read with PostGIS's (E)WKT rules (`'POINT(1 2 3)'` is a POINT Z), not as OGC WKT. A
/// `SRID=n;` prefix can't become the column's CRS here, so an SRID other than the field's is an
/// error rather than dropped; a `::geometry` cast keeps it.
pub(crate) fn geometries_from_array(
    array: &ArrayRef,
    field: &Field,
) -> GeoDataFusionResult<Arc<dyn GeoArrowArray>> {
    let is_text = matches!(
        array.data_type(),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View
    );
    if !is_text || field.extension_type_name().is_some() {
        return Ok(from_arrow_array(array, field)?);
    }
    let metadata = input_metadata(field);
    let field_srid = crs_to_srid(metadata.crs()).unwrap_or(SRID_UNKNOWN);
    let text = cast(array, &DataType::Utf8)?;
    let mut builder = WkbBuilder::<i32>::new(WkbType::new(metadata));
    for value in text.as_string::<i32>() {
        let Some(value) = value else {
            builder.push_geometry(None::<&Wkt<f64>>)?;
            continue;
        };
        let (srid, geometry) =
            parse_ewkt(value).map_err(|e| exec_datafusion_err!("geometry: {e}"))?;
        if let Some(srid) = srid.filter(|srid| *srid != field_srid) {
            return Err(exec_datafusion_err!(
                "geometry: text with SRID={srid} needs a ::geometry cast to keep its SRID"
            )
            .into());
        }
        builder.push_geometry(Some(&geometry))?;
    }
    Ok(Arc::new(builder.finish()))
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
