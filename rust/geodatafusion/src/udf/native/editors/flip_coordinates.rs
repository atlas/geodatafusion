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

/// Returns a version of a geometry with X and Y axis flipped.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry with X and Y swapped, for example to fix coordinates written as latitude, longitude. Z and M are unchanged.",
    syntax_example = "ST_FlipCoordinates(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_swapordinates")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct FlipCoordinates;

impl FlipCoordinates {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FlipCoordinates {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for FlipCoordinates {
    fn name(&self) -> &str {
        "st_flipcoordinates"
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
        Ok(flip_coordinates_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn flip_coordinates_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(
        geometries.as_ref(),
        &FlipCoordinatesKernel,
        &args.return_field,
    )?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct FlipCoordinatesKernel;

impl GeometryKernel for FlipCoordinatesKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(map_coords(geom, &|c| Coord {
            x: c.y,
            y: c.x,
            ..c
        })))
    }
}
