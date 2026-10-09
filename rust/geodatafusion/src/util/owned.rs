//! Owned copies of geometries: for further geometry arguments, read by row index, and for
//! returning an input unchanged.

use arrow_array::Array;
use arrow_schema::{DataType, Field};
use datafusion::logical_expr::ColumnarValue;
use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    LineTrait, MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
    RectTrait, TriangleTrait,
};
use geoarrow_array::array::from_arrow_array;
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::{m, z};

/// An owned copy of a geometry, Z and M included.
pub(crate) fn to_owned_geometry(geom: &impl GeometryTrait<T = f64>) -> Wkt<f64> {
    let dim = dimension(geom.dim());
    match geom.as_type() {
        GeometryType::Point(point) => Wkt::Point(owned_point(point, dim)),
        GeometryType::LineString(line) => Wkt::LineString(owned_line_string(line, dim)),
        GeometryType::Polygon(polygon) => Wkt::Polygon(owned_polygon(polygon, dim)),
        GeometryType::MultiPoint(points) => Wkt::MultiPoint(MultiPoint::new(
            points
                .points()
                .map(|point| owned_point(&point, dim))
                .collect(),
            dim,
        )),
        GeometryType::MultiLineString(lines) => Wkt::MultiLineString(MultiLineString::new(
            lines
                .line_strings()
                .map(|line| owned_line_string(&line, dim))
                .collect(),
            dim,
        )),
        GeometryType::MultiPolygon(polygons) => Wkt::MultiPolygon(MultiPolygon::new(
            polygons
                .polygons()
                .map(|polygon| owned_polygon(&polygon, dim))
                .collect(),
            dim,
        )),
        GeometryType::GeometryCollection(collection) => {
            Wkt::GeometryCollection(GeometryCollection::new(
                collection
                    .geometries()
                    .map(|member| to_owned_geometry(&member))
                    .collect(),
                dim,
            ))
        }
        // As the polygon or linestring PostGIS would see.
        GeometryType::Rect(rect) => {
            let (min, max) = (owned_coord(&rect.min()), owned_coord(&rect.max()));
            let corner = |x, y| Coord { x, y, ..min };
            let ring = vec![
                min,
                corner(max.x, min.y),
                corner(max.x, max.y),
                corner(min.x, max.y),
                min,
            ];
            Wkt::Polygon(Polygon::new(vec![LineString::new(ring, dim)], dim))
        }
        GeometryType::Triangle(triangle) => {
            let coords: Vec<Coord<f64>> = triangle.coords().iter().map(owned_coord).collect();
            let ring = coords.iter().chain(coords.first()).cloned().collect();
            Wkt::Polygon(Polygon::new(vec![LineString::new(ring, dim)], dim))
        }
        GeometryType::Line(line) => Wkt::LineString(LineString::new(
            vec![owned_coord(&line.start()), owned_coord(&line.end())],
            dim,
        )),
    }
}

/// An owned copy of a point that is part of a geometry with dimension `dim`.
pub(crate) fn point_to_owned(point: &impl PointTrait<T = f64>, dim: Dimensions) -> Wkt<f64> {
    Wkt::Point(owned_point(point, dimension(dim)))
}

/// An owned copy of a linestring (or ring) that is part of a geometry with dimension `dim`.
pub(crate) fn line_string_to_owned(
    line: &impl LineStringTrait<T = f64>,
    dim: Dimensions,
) -> Wkt<f64> {
    Wkt::LineString(owned_line_string(line, dimension(dim)))
}

/// An owned copy of a polygon that is part of a geometry with dimension `dim`.
pub(crate) fn polygon_to_owned(polygon: &impl PolygonTrait<T = f64>, dim: Dimensions) -> Wkt<f64> {
    Wkt::Polygon(owned_polygon(polygon, dimension(dim)))
}

fn dimension(dim: Dimensions) -> Dimension {
    match dim {
        Dimensions::Xyz => Dimension::XYZ,
        Dimensions::Xym => Dimension::XYM,
        Dimensions::Xyzm => Dimension::XYZM,
        _ => Dimension::XY,
    }
}

fn owned_coord(coord: &impl CoordTrait<T = f64>) -> Coord<f64> {
    Coord {
        x: coord.x(),
        y: coord.y(),
        z: z(coord),
        m: m(coord),
    }
}

fn owned_point(point: &impl PointTrait<T = f64>, dim: Dimension) -> Point<f64> {
    Point::new(point.coord().map(|coord| owned_coord(&coord)), dim)
}

fn owned_line_string(line: &impl LineStringTrait<T = f64>, dim: Dimension) -> LineString<f64> {
    LineString::new(
        line.coords().map(|coord| owned_coord(&coord)).collect(),
        dim,
    )
}

fn owned_polygon(polygon: &impl PolygonTrait<T = f64>, dim: Dimension) -> Polygon<f64> {
    let rings = polygon
        .exterior()
        .into_iter()
        .chain(polygon.interiors())
        .map(|ring| owned_line_string(&ring, dim))
        .collect();
    Polygon::new(rings, dim)
}

/// A geometry argument as owned geometries, one per row. A constant is copied once.
pub(crate) struct OwnedColumn {
    rows: Vec<Option<Wkt<f64>>>,
    is_scalar: bool,
}

impl OwnedColumn {
    pub(crate) fn try_new(
        value: &ColumnarValue,
        field: &Field,
        number_rows: usize,
    ) -> GeoDataFusionResult<Self> {
        let is_scalar = matches!(value, ColumnarValue::Scalar(_));
        let array = value.to_array(if is_scalar { 1 } else { number_rows })?;
        let rows = if array.data_type() == &DataType::Null {
            vec![None; array.len()]
        } else {
            map_geometry(from_arrow_array(&array, field)?.as_ref(), &ToOwned)?
        };
        Ok(Self { rows, is_scalar })
    }

    /// The geometry in row `row`, or `None` for SQL NULL.
    pub(crate) fn get(&self, row: usize) -> Option<&Wkt<f64>> {
        let index = if self.is_scalar { 0 } else { row };
        self.rows.get(index).and_then(Option::as_ref)
    }

    /// The geometries of the rows; one for a constant.
    pub(crate) fn rows(&self) -> &[Option<Wkt<f64>>] {
        &self.rows
    }

    /// The row index into [`Self::rows`] for row `row`.
    pub(crate) fn index(&self, row: usize) -> usize {
        if self.is_scalar { 0 } else { row }
    }
}

/// Owned copies of the geometries of an array, by [`map_geometry`].
pub(crate) struct ToOwned;

impl GeometryKernel for ToOwned {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(to_owned_geometry(geom)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::udf::native::io::util::wkt::{WktFlavor, parse_ewkt, write_wkt};

    #[test]
    fn test_to_owned_geometry_keeps_z_and_m() {
        for wkt in [
            "POINT ZM (1 2 3 4)",
            "LINESTRING M (0 0 1,1 1 2)",
            "POLYGON Z ((0 0 1,1 0 1,1 1 1,0 0 1))",
            "MULTIPOINT((1 2),EMPTY)",
            "GEOMETRYCOLLECTION M (POINT M (1 2 3),LINESTRING M EMPTY)",
            "POLYGON EMPTY",
        ] {
            let (_, geom) = parse_ewkt(wkt).unwrap();
            let mut out = String::new();
            write_wkt(&mut out, &to_owned_geometry(&geom), WktFlavor::Iso, 15);
            assert_eq!(out, wkt);
        }
    }
}
