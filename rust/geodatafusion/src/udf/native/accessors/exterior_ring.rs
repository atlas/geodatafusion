use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, PolygonTrait};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{empty_line_string, line_string_to_owned, to_owned_geometry};
use crate::util::signature::single_geometry;

/// Returns a LineString representing the exterior ring of a Polygon.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the exterior ring of a POLYGON as a LINESTRING, and LINESTRING EMPTY for an empty polygon. Returns NULL for any other geometry type, including a MULTIPOLYGON.",
    syntax_example = "ST_ExteriorRing(a_polygon)",
    argument(name = "a_polygon", description = "geometry"),
    related_udf(name = "st_interiorringn")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ExteriorRing;

impl ExteriorRing {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExteriorRing {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ExteriorRing {
    fn name(&self) -> &str {
        "st_exteriorring"
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
        Ok(exterior_ring_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn exterior_ring_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &ExteriorRingKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ExteriorRingKernel;

impl GeometryKernel for ExteriorRingKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let dim = geom.dim();
        Ok(match geom.as_type() {
            GeometryType::Polygon(polygon) => Some(match polygon.exterior() {
                Some(ring) => line_string_to_owned(&ring, dim),
                None => empty_line_string(dim),
            }),
            GeometryType::Rect(_) | GeometryType::Triangle(_) => {
                let Wkt::Polygon(polygon) = to_owned_geometry(geom) else {
                    return Ok(None);
                };
                polygon
                    .exterior()
                    .map(|ring| line_string_to_owned(ring, dim))
            }
            _ => None,
        })
    }
}
