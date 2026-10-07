//! ST_Area.

use std::sync::Arc;

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiPolygonTrait, PolygonTrait, RectTrait, TriangleTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Returns the area of a geometry.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the area of a polygonal geometry: the area of each polygon's shell minus the areas of its holes, summed over the parts of a collection. Points and lines have zero area, as does an empty geometry. Z and M are ignored.",
    syntax_example = "ST_Area(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Area;

impl Area {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Area {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Area {
    fn name(&self) -> &str {
        "st_area"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(area_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn area_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &AreaKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AreaKernel;

impl GeometryKernel for AreaKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(area(geom)))
    }
}

pub(crate) fn area(geom: &impl GeometryTrait<T = f64>) -> f64 {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_area(polygon),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_area(&polygon))
            .sum(),
        GeometryType::GeometryCollection(collection) => {
            collection.geometries().map(|member| area(&member)).sum()
        }
        GeometryType::Rect(rect) => {
            (rect.max().x() - rect.min().x()) * (rect.max().y() - rect.min().y())
        }
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| (coord.x(), coord.y()));
            ring_area([a, b, c, a].into_iter())
        }
        GeometryType::Point(_)
        | GeometryType::LineString(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::MultiLineString(_)
        | GeometryType::Line(_) => 0.0,
    }
}

/// The shell's area minus the holes' areas. PostGIS doesn't clamp it, so a hole larger than
/// its shell gives a negative area.
fn polygon_area(polygon: &impl PolygonTrait<T = f64>) -> f64 {
    let Some(shell) = polygon.exterior() else {
        return 0.0;
    };
    polygon
        .interiors()
        .fold(ring_area(xy(&shell)), |area, hole| {
            area - ring_area(xy(&hole))
        })
}

fn xy(ring: &impl LineStringTrait<T = f64>) -> impl Iterator<Item = (f64, f64)> {
    ring.coords().map(|coord| (coord.x(), coord.y()))
}

/// The unsigned area of a closed ring.
///
/// This is the shoelace formula with the coordinates shifted by the first X, as JTS computes
/// it (`Area.ofRingSigned`). The shift keeps precision for coordinates far from the origin,
/// and this summation order agrees with PostGIS to the last digit, where geo's doesn't.
fn ring_area(mut ring: impl Iterator<Item = (f64, f64)>) -> f64 {
    let (Some(first), Some(mut current)) = (ring.next(), ring.next()) else {
        return 0.0;
    };
    let x0 = first.0;
    let mut previous_y = first.1;
    let mut sum = 0.0;
    for next in ring {
        sum += (current.0 - x0) * (previous_y - next.1);
        previous_y = current.1;
        current = next;
    }
    (sum / 2.0).abs()
}
