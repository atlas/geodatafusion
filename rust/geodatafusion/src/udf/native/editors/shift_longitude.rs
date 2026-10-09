use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_coords;
use crate::util::signature::single_geometry;

/// Shifts the longitude coordinates of a geometry between -180..180 and 0..360.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Shifts each longitude (X) by 360 degrees once: a negative X has 360 added, and an X above 180 has 360 subtracted. This moves geometries between the -180..180 and 0..360 ranges. Y, Z and M are unchanged.",
    syntax_example = "ST_ShiftLongitude(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_wrapx")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ShiftLongitude;

impl ShiftLongitude {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShiftLongitude {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ShiftLongitude {
    fn name(&self) -> &str {
        "st_shiftlongitude"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
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

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(shift_longitude_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn shift_longitude_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(
        geometries.as_ref(),
        &ShiftLongitudeKernel,
        &args.return_field,
    )?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ShiftLongitudeKernel;

impl GeometryKernel for ShiftLongitudeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(map_coords(geom, &|c| Coord {
            x: shift_longitude(c.x),
            ..c
        })))
    }
}

/// One shift by 360 degrees, as PostGIS does it: values outside [-360, 540] stay out of range.
fn shift_longitude(x: f64) -> f64 {
    if x < 0.0 {
        x + 360.0
    } else if x > 180.0 {
        x - 360.0
    } else {
        x
    }
}
