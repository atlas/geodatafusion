use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo::{Area, Validation};
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Validation"),
    description = "Tests if an ST_Geometry value is well-formed and valid in 2D according to the OGC rules. An empty geometry is valid, and empty parts are ignored. A ring with zero area is invalid.",
    syntax_example = "ST_IsValid(geomA)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValid;

impl IsValid {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for IsValid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValid {
    fn name(&self) -> &str {
        "st_isvalid"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_valid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_valid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsValidKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsValidKernel;

impl GeometryKernel for IsValidKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // An EMPTY geometry is valid.
        let Some(geom) = to_geo(geom) else {
            return Ok(Some(true));
        };
        Ok(Some(geom.is_valid() && !has_zero_area_ring(&geom)))
    }
}

/// A `geo` geometry without the EMPTY parts, which PostGIS ignores and `geo` can't represent
/// (`POINT EMPTY`), or `None` if nothing is left.
fn to_geo(geom: &impl GeometryTrait<T = f64>) -> Option<geo::Geometry> {
    let geom = match geom.as_type() {
        GeometryType::Point(point) => geo::Geometry::Point(to_geo_point(point)?),
        GeometryType::LineString(line) => geo::Geometry::LineString(to_geo_line_string(line)?),
        GeometryType::Polygon(polygon) => geo::Geometry::Polygon(to_geo_polygon(polygon)?),
        GeometryType::MultiPoint(points) => geo::Geometry::MultiPoint(geo::MultiPoint::new(
            non_empty(points.points().filter_map(|point| to_geo_point(&point)))?,
        )),
        GeometryType::MultiLineString(lines) => {
            geo::Geometry::MultiLineString(geo::MultiLineString::new(non_empty(
                lines
                    .line_strings()
                    .filter_map(|line| to_geo_line_string(&line)),
            )?))
        }
        GeometryType::MultiPolygon(polygons) => {
            geo::Geometry::MultiPolygon(geo::MultiPolygon::new(non_empty(
                polygons
                    .polygons()
                    .filter_map(|polygon| to_geo_polygon(&polygon)),
            )?))
        }
        GeometryType::GeometryCollection(collection) => {
            geo::Geometry::GeometryCollection(geo::GeometryCollection::new_from(non_empty(
                collection.geometries().filter_map(|member| to_geo(&member)),
            )?))
        }
        GeometryType::Rect(rect) => geo::Geometry::Rect(geo::Rect::new(
            to_geo_coord(&rect.min()),
            to_geo_coord(&rect.max()),
        )),
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| to_geo_coord(&coord));
            geo::Geometry::Triangle(geo::Triangle::new(a, b, c))
        }
        GeometryType::Line(line) => geo::Geometry::Line(geo::Line::new(
            to_geo_coord(&line.start()),
            to_geo_coord(&line.end()),
        )),
    };
    Some(geom)
}

fn non_empty<T>(parts: impl Iterator<Item = T>) -> Option<Vec<T>> {
    let parts: Vec<T> = parts.collect();
    (!parts.is_empty()).then_some(parts)
}

fn to_geo_coord(coord: &impl CoordTrait<T = f64>) -> geo::Coord {
    geo::coord! { x: coord.x(), y: coord.y() }
}

fn to_geo_point(point: &impl PointTrait<T = f64>) -> Option<geo::Point> {
    let coord = point.coord()?;
    // WKB writes POINT EMPTY as NaN coordinates.
    if coord.x().is_nan() && coord.y().is_nan() {
        return None;
    }
    Some(geo::Point(to_geo_coord(&coord)))
}

fn to_geo_line_string(line: &impl LineStringTrait<T = f64>) -> Option<geo::LineString> {
    let coords: Vec<geo::Coord> = line.coords().map(|coord| to_geo_coord(&coord)).collect();
    (!coords.is_empty()).then(|| geo::LineString::new(coords))
}

fn to_geo_polygon(polygon: &impl PolygonTrait<T = f64>) -> Option<geo::Polygon> {
    let shell = to_geo_line_string(&polygon.exterior()?)?;
    let holes = polygon
        .interiors()
        .filter_map(|hole| to_geo_line_string(&hole))
        .collect();
    Some(geo::Polygon::new(shell, holes))
}

/// Whether a polygon has a ring with zero area, which PostGIS finds invalid and `geo` doesn't.
fn has_zero_area_ring(geom: &geo::Geometry) -> bool {
    let ring_has_zero_area =
        |ring: &geo::LineString| geo::Polygon::new(ring.clone(), vec![]).unsigned_area() == 0.0;
    let polygon_has_zero_area_ring = |polygon: &geo::Polygon| {
        std::iter::once(polygon.exterior())
            .chain(polygon.interiors())
            .any(ring_has_zero_area)
    };
    match geom {
        geo::Geometry::Polygon(polygon) => polygon_has_zero_area_ring(polygon),
        geo::Geometry::MultiPolygon(polygons) => polygons.iter().any(polygon_has_zero_area_ring),
        geo::Geometry::GeometryCollection(collection) => collection.iter().any(has_zero_area_ring),
        _ => false,
    }
}
