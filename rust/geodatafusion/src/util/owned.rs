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
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometries_from_array;
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

/// A copy of `geom` with `f` applied to every coordinate, keeping its dimension.
pub(crate) fn map_coords(
    geom: &impl GeometryTrait<T = f64>,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> Wkt<f64> {
    map_wkt_coords(to_owned_geometry(geom), None, f)
}

/// A copy of `geom` in dimension `dim`, with `f` applied to every coordinate. `f` returns
/// coordinates with the ordinates of `dim`.
pub(crate) fn map_coords_to_dimension(
    geom: &impl GeometryTrait<T = f64>,
    dim: Dimension,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> Wkt<f64> {
    map_wkt_coords(to_owned_geometry(geom), Some(dim), f)
}

fn map_wkt_coords(
    geom: Wkt<f64>,
    dim: Option<Dimension>,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> Wkt<f64> {
    match geom {
        Wkt::Point(point) => Wkt::Point(map_point(point, dim, f)),
        Wkt::LineString(line) => Wkt::LineString(map_line_string(line, dim, f)),
        Wkt::Polygon(polygon) => Wkt::Polygon(map_polygon(polygon, dim, f)),
        Wkt::MultiPoint(points) => {
            let (points, own_dim) = points.into_inner();
            Wkt::MultiPoint(MultiPoint::new(
                points.into_iter().map(|p| map_point(p, dim, f)).collect(),
                dim.unwrap_or(own_dim),
            ))
        }
        Wkt::MultiLineString(lines) => {
            let (lines, own_dim) = lines.into_inner();
            Wkt::MultiLineString(MultiLineString::new(
                lines
                    .into_iter()
                    .map(|line| map_line_string(line, dim, f))
                    .collect(),
                dim.unwrap_or(own_dim),
            ))
        }
        Wkt::MultiPolygon(polygons) => {
            let (polygons, own_dim) = polygons.into_inner();
            Wkt::MultiPolygon(MultiPolygon::new(
                polygons
                    .into_iter()
                    .map(|polygon| map_polygon(polygon, dim, f))
                    .collect(),
                dim.unwrap_or(own_dim),
            ))
        }
        Wkt::GeometryCollection(collection) => {
            let (members, own_dim) = collection.into_inner();
            Wkt::GeometryCollection(GeometryCollection::new(
                members
                    .into_iter()
                    .map(|member| map_wkt_coords(member, dim, f))
                    .collect(),
                dim.unwrap_or(own_dim),
            ))
        }
    }
}

fn map_point(
    point: Point<f64>,
    dim: Option<Dimension>,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> Point<f64> {
    let (coord, own_dim) = point.into_inner();
    Point::new(coord.map(f), dim.unwrap_or(own_dim))
}

fn map_line_string(
    line: LineString<f64>,
    dim: Option<Dimension>,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> LineString<f64> {
    let (coords, own_dim) = line.into_inner();
    LineString::new(coords.into_iter().map(f).collect(), dim.unwrap_or(own_dim))
}

fn map_polygon(
    polygon: Polygon<f64>,
    dim: Option<Dimension>,
    f: &impl Fn(Coord<f64>) -> Coord<f64>,
) -> Polygon<f64> {
    let (rings, own_dim) = polygon.into_inner();
    Polygon::new(
        rings
            .into_iter()
            .map(|ring| map_line_string(ring, dim, f))
            .collect(),
        dim.unwrap_or(own_dim),
    )
}

/// Owned copies of the points, linestrings and polygons a geometry is made of, in order, through
/// MULTI* geometries and nested collections. Empty parts are included; an empty MULTI* or
/// collection has none.
pub(crate) fn owned_atoms(geom: &impl GeometryTrait<T = f64>) -> Vec<Wkt<f64>> {
    let dim = dimension(geom.dim());
    match geom.as_type() {
        GeometryType::MultiPoint(points) => points
            .points()
            .map(|point| Wkt::Point(owned_point(&point, dim)))
            .collect(),
        GeometryType::MultiLineString(lines) => lines
            .line_strings()
            .map(|line| Wkt::LineString(owned_line_string(&line, dim)))
            .collect(),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| Wkt::Polygon(owned_polygon(&polygon, dim)))
            .collect(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .flat_map(|member| owned_atoms(&member))
            .collect(),
        _ => vec![to_owned_geometry(geom)],
    }
}

/// The role of a linestring in a geometry, for [`map_line_strings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinePart {
    Line,
    ExteriorRing,
    InteriorRing,
}

/// A copy of `geom` with `f` applied to the coordinates of every linestring and polygon ring.
/// Points are copied unchanged.
pub(crate) fn map_line_strings(
    geom: &impl GeometryTrait<T = f64>,
    f: &impl Fn(LinePart, Vec<Coord<f64>>) -> Vec<Coord<f64>>,
) -> Wkt<f64> {
    map_wkt_line_strings(to_owned_geometry(geom), f)
}

fn map_wkt_line_strings(
    geom: Wkt<f64>,
    f: &impl Fn(LinePart, Vec<Coord<f64>>) -> Vec<Coord<f64>>,
) -> Wkt<f64> {
    let line = |line: LineString<f64>, part| {
        let (coords, dim) = line.into_inner();
        LineString::new(f(part, coords), dim)
    };
    let polygon = |polygon: Polygon<f64>| {
        let (rings, dim) = polygon.into_inner();
        let rings = rings
            .into_iter()
            .enumerate()
            .map(|(index, ring)| {
                let part = match index {
                    0 => LinePart::ExteriorRing,
                    _ => LinePart::InteriorRing,
                };
                line(ring, part)
            })
            .collect();
        Polygon::new(rings, dim)
    };
    match geom {
        Wkt::Point(_) | Wkt::MultiPoint(_) => geom,
        Wkt::LineString(l) => Wkt::LineString(line(l, LinePart::Line)),
        Wkt::Polygon(p) => Wkt::Polygon(polygon(p)),
        Wkt::MultiLineString(lines) => {
            let (lines, dim) = lines.into_inner();
            Wkt::MultiLineString(MultiLineString::new(
                lines.into_iter().map(|l| line(l, LinePart::Line)).collect(),
                dim,
            ))
        }
        Wkt::MultiPolygon(polygons) => {
            let (polygons, dim) = polygons.into_inner();
            Wkt::MultiPolygon(MultiPolygon::new(
                polygons.into_iter().map(polygon).collect(),
                dim,
            ))
        }
        Wkt::GeometryCollection(collection) => {
            let (members, dim) = collection.into_inner();
            Wkt::GeometryCollection(GeometryCollection::new(
                members
                    .into_iter()
                    .map(|member| map_wkt_line_strings(member, f))
                    .collect(),
                dim,
            ))
        }
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

/// An empty linestring with dimension `dim`.
pub(crate) fn empty_line_string(dim: Dimensions) -> Wkt<f64> {
    Wkt::LineString(LineString::new(vec![], dimension(dim)))
}

/// The WKT dimension of a geo-traits dimension; unknown dimensions are 2D.
pub(crate) fn dimension(dim: Dimensions) -> Dimension {
    match dim {
        Dimensions::Xyz => Dimension::XYZ,
        Dimensions::Xym => Dimension::XYM,
        Dimensions::Xyzm => Dimension::XYZM,
        _ => Dimension::XY,
    }
}

/// An owned copy of a coordinate, Z and M included.
pub(crate) fn owned_coord(coord: &impl CoordTrait<T = f64>) -> Coord<f64> {
    Coord {
        x: coord.x(),
        y: coord.y(),
        z: z(coord),
        m: m(coord),
    }
}

/// An owned copy of a point that is part of a geometry with WKT dimension `dim`.
pub(crate) fn owned_point(point: &impl PointTrait<T = f64>, dim: Dimension) -> Point<f64> {
    Point::new(point.coord().map(|coord| owned_coord(&coord)), dim)
}

/// An owned copy of a linestring that is part of a geometry with WKT dimension `dim`.
pub(crate) fn owned_line_string(
    line: &impl LineStringTrait<T = f64>,
    dim: Dimension,
) -> LineString<f64> {
    LineString::new(
        line.coords().map(|coord| owned_coord(&coord)).collect(),
        dim,
    )
}

/// An owned copy of a polygon that is part of a geometry with WKT dimension `dim`.
pub(crate) fn owned_polygon(polygon: &impl PolygonTrait<T = f64>, dim: Dimension) -> Polygon<f64> {
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
            map_geometry(geometries_from_array(&array, field)?.as_ref(), &ToOwned)?
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
    use crate::udf::native::io::util::wkt::{WktFlavor, write_wkt};
    use crate::util::ewkt::parse_ewkt;

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
