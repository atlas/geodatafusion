//! ST_IsPolygonCW and ST_IsPolygonCCW: the orientation of polygon rings.

use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiPolygonTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::single_geometry;

/// Tests if Polygons have exterior rings oriented clockwise and interior rings oriented
/// counter-clockwise.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if every polygon in the geometry has a clockwise exterior ring and counter-clockwise interior rings. Geometries without polygons, and empty geometries, return true. A ring with no area is neither clockwise nor counter-clockwise.",
    syntax_example = "ST_IsPolygonCW(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_ispolygonccw")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsPolygonCW;

impl IsPolygonCW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsPolygonCW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsPolygonCW {
    fn name(&self) -> &str {
        "st_ispolygoncw"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_polygon_oriented_impl(args, Orientation::Clockwise)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Tests if Polygons have exterior rings oriented counter-clockwise and interior rings oriented
/// clockwise.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if every polygon in the geometry has a counter-clockwise exterior ring and clockwise interior rings. Geometries without polygons, and empty geometries, return true. A ring with no area is neither clockwise nor counter-clockwise.",
    syntax_example = "ST_IsPolygonCCW(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_ispolygoncw")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsPolygonCCW;

impl IsPolygonCCW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsPolygonCCW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsPolygonCCW {
    fn name(&self) -> &str {
        "st_ispolygonccw"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_polygon_oriented_impl(
            args,
            Orientation::CounterClockwise,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_polygon_oriented_impl(
    args: ScalarFunctionArgs,
    orientation: Orientation,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &orientation)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// The orientation the exterior rings must have; interior rings must have the other one.
#[derive(Debug, Clone, Copy)]
enum Orientation {
    Clockwise,
    CounterClockwise,
}

impl GeometryKernel for Orientation {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        Ok(Some(self.geometry_has(geom)))
    }
}

impl Orientation {
    fn geometry_has(self, geom: &impl GeometryTrait<T = f64>) -> bool {
        match geom.as_type() {
            GeometryType::Polygon(polygon) => self.polygon_has(polygon),
            GeometryType::MultiPolygon(polygons) => polygons
                .polygons()
                .all(|polygon| self.polygon_has(&polygon)),
            GeometryType::GeometryCollection(collection) => collection
                .geometries()
                .all(|member| self.geometry_has(&member)),
            // As the polygon PostGIS would see.
            GeometryType::Rect(_) | GeometryType::Triangle(_) => {
                self.geometry_has(&to_owned_geometry(geom))
            }
            _ => true,
        }
    }

    fn polygon_has(self, polygon: &impl PolygonTrait<T = f64>) -> bool {
        let (exterior_sign, interior_sign) = match self {
            Orientation::Clockwise => (-1.0, 1.0),
            Orientation::CounterClockwise => (1.0, -1.0),
        };
        // An empty polygon has no rings to orient.
        polygon.exterior().is_none_or(|ring| {
            ring.num_coords() == 0 || ring_signed_area(&ring) * exterior_sign > 0.0
        }) && polygon
            .interiors()
            .all(|ring| ring_signed_area(&ring) * interior_sign > 0.0)
    }
}

/// Twice the signed area of a ring (the shoelace formula): positive when counter-clockwise,
/// negative when clockwise, and zero when it has no area. Coordinates are taken relative to the
/// first one, to lose less precision far from the origin.
fn ring_signed_area(ring: &impl LineStringTrait<T = f64>) -> f64 {
    let Some(origin) = ring.coord(0) else {
        return 0.0;
    };
    let (x0, y0) = (origin.x(), origin.y());
    let coords: Vec<(f64, f64)> = ring.coords().map(|c| (c.x() - x0, c.y() - y0)).collect();
    coords
        .windows(2)
        .map(|pair| pair[0].0 * pair[1].1 - pair[1].0 * pair[0].1)
        .sum()
}
