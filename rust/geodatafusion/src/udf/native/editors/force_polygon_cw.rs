//! ST_ForcePolygonCW (alias ST_ForceRHR) and ST_ForcePolygonCCW: polygon rings in a given
//! orientation.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::orientation::ring_signed_area;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{LinePart, map_line_strings};
use crate::util::signature::single_geometry;

static FORCE_POLYGON_CW_ALIASES: LazyLock<Vec<String>> =
    LazyLock::new(|| vec!["st_forcerhr".to_string()]);

/// Orients all exterior rings clockwise and all interior rings counter-clockwise.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Orients the exterior rings of polygons clockwise and their interior rings counter-clockwise, reversing the rings that aren't. Other geometries are unchanged. As in PostGIS, a ring with no area counts as counter-clockwise. ST_ForceRHR (the right-hand rule) is an alias.",
    syntax_example = "ST_ForcePolygonCW(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_forcepolygonccw"),
    related_udf(name = "st_ispolygoncw")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ForcePolygonCW;

impl ForcePolygonCW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ForcePolygonCW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ForcePolygonCW {
    fn name(&self) -> &str {
        "st_forcepolygoncw"
    }

    fn aliases(&self) -> &[String] {
        &FORCE_POLYGON_CW_ALIASES
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
        Ok(force_polygon_impl(args, Orientation::Clockwise)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Orients all exterior rings counter-clockwise and all interior rings clockwise.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Orients the exterior rings of polygons counter-clockwise and their interior rings clockwise, reversing the rings that aren't. Other geometries are unchanged. As in PostGIS, a ring with no area counts as counter-clockwise.",
    syntax_example = "ST_ForcePolygonCCW(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_forcepolygoncw"),
    related_udf(name = "st_ispolygonccw")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ForcePolygonCCW;

impl ForcePolygonCCW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ForcePolygonCCW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ForcePolygonCCW {
    fn name(&self) -> &str {
        "st_forcepolygonccw"
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
        Ok(force_polygon_impl(args, Orientation::CounterClockwise)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn force_polygon_impl(
    args: ScalarFunctionArgs,
    orientation: Orientation,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &orientation, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// The orientation exterior rings are given; interior rings get the other one.
#[derive(Debug, Clone, Copy)]
enum Orientation {
    Clockwise,
    CounterClockwise,
}

impl GeometryKernel for Orientation {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(map_line_strings(geom, &|part, mut coords| {
            let want_clockwise = match (part, self) {
                (LinePart::Line, _) => return coords,
                (LinePart::ExteriorRing, Orientation::Clockwise)
                | (LinePart::InteriorRing, Orientation::CounterClockwise) => true,
                (LinePart::ExteriorRing, Orientation::CounterClockwise)
                | (LinePart::InteriorRing, Orientation::Clockwise) => false,
            };
            if is_clockwise(&coords) != want_clockwise {
                coords.reverse();
            }
            coords
        })))
    }
}

/// Whether a ring is clockwise: a ring with no area isn't, as in PostGIS.
fn is_clockwise(coords: &[Coord<f64>]) -> bool {
    // The dimension doesn't matter for the area.
    let ring = LineString::new(coords.to_vec(), Dimension::XY);
    ring_signed_area(&ring) < 0.0
}
