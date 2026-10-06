//! Per-geometry functions shared by both loop styles, so the styles differ only in how rows are
//! read.

use datafusion::error::{DataFusionError, Result};
use geo_traits::to_geo::{
    ToGeoLine, ToGeoLineString, ToGeoMultiLineString, ToGeoMultiPoint, ToGeoMultiPolygon,
    ToGeoPoint, ToGeoPolygon, ToGeoRect, ToGeoTriangle,
};
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};

pub fn ext<E: std::error::Error + Send + Sync + 'static>(e: E) -> DataFusionError {
    DataFusionError::External(Box::new(e))
}

/// ST_X: the X of a point; NULL for POINT EMPTY; an error for other types (PostGIS, D16).
#[inline]
pub fn x_of(g: &impl GeometryTrait<T = f64>) -> Result<Option<f64>> {
    match g.as_type() {
        GeometryType::Point(p) => Ok(p.coord().map(|c| c.x())),
        _ => Err(DataFusionError::Execution(
            "st_x: argument must be a point".to_string(),
        )),
    }
}

/// ST_NPoints.
#[inline]
pub fn npoints(g: &impl GeometryTrait<T = f64>) -> i32 {
    fn ls(l: &impl LineStringTrait) -> usize {
        l.num_coords()
    }
    fn poly(p: &impl PolygonTrait) -> usize {
        p.exterior().map(|r| ls(&r)).unwrap_or(0) + p.interiors().map(|r| ls(&r)).sum::<usize>()
    }
    fn geom(g: &impl GeometryTrait<T = f64>) -> usize {
        match g.as_type() {
            GeometryType::Point(p) => p.coord().is_some() as usize,
            GeometryType::LineString(l) => ls(l),
            GeometryType::Polygon(p) => poly(p),
            GeometryType::MultiPoint(mp) => mp.points().filter(|p| p.coord().is_some()).count(),
            GeometryType::MultiLineString(m) => m.line_strings().map(|l| ls(&l)).sum(),
            GeometryType::MultiPolygon(m) => m.polygons().map(|p| poly(&p)).sum(),
            GeometryType::GeometryCollection(gc) => gc.geometries().map(|c| geom(&c)).sum(),
            GeometryType::Rect(_) => 5,
            GeometryType::Triangle(_) => 4,
            GeometryType::Line(_) => 2,
        }
    }
    geom(g) as i32
}

/// ST_IsEmpty (copy of geodatafusion's `is_geometry_topologically_empty`, which is crate-private).
#[inline]
pub fn is_empty(geom: &impl GeometryTrait<T = f64>) -> bool {
    match geom.as_type() {
        GeometryType::Point(p) => p.coord().is_none(),
        GeometryType::LineString(ls) => ls.num_coords() == 0,
        GeometryType::Polygon(p) => p.exterior().is_none_or(|ring| ring.num_coords() == 0),
        GeometryType::MultiPoint(mp) => mp.points().all(|p| p.coord().is_none()),
        GeometryType::MultiLineString(mls) => mls.line_strings().all(|ls| ls.num_coords() == 0),
        GeometryType::MultiPolygon(mp) => mp
            .polygons()
            .all(|p| p.exterior().is_none_or(|ring| ring.num_coords() == 0)),
        GeometryType::GeometryCollection(gc) => gc.geometries().all(|child| is_empty(&child)),
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => false,
    }
}

/// G2's `GeoValue` (plans/g2-geo.md §4).
#[derive(Debug, Clone)]
pub enum GeoValue {
    Null,
    Empty,
    Geometry(geo::Geometry),
}

/// G2's `geometry_to_geo`: `Empty` for a topologically empty geometry.
#[inline]
pub fn to_geo_value(g: &impl GeometryTrait<T = f64>) -> Result<GeoValue> {
    if is_empty(g) {
        Ok(GeoValue::Empty)
    } else {
        Ok(GeoValue::Geometry(geometry_to_geo(g)?))
    }
}

/// Same implementation as `geoarrow_expr_geo::util::to_geo::geometry_to_geo`, owned here.
pub fn geometry_to_geo(geometry: &impl GeometryTrait<T = f64>) -> Result<geo::Geometry> {
    use GeometryType::*;
    let empty_point = || DataFusionError::Execution("empty point".to_string());
    Ok(match geometry.as_type() {
        Point(g) => geo::Geometry::Point(g.try_to_point().ok_or_else(empty_point)?),
        LineString(g) => geo::Geometry::LineString(g.to_line_string()),
        Polygon(g) => geo::Geometry::Polygon(g.to_polygon()),
        MultiPoint(g) => geo::Geometry::MultiPoint(g.try_to_multi_point().ok_or_else(empty_point)?),
        MultiLineString(g) => geo::Geometry::MultiLineString(g.to_multi_line_string()),
        MultiPolygon(g) => geo::Geometry::MultiPolygon(g.to_multi_polygon()),
        GeometryCollection(g) => geo::Geometry::GeometryCollection(geo::GeometryCollection::new_from(
            g.geometries()
                .map(|c| geometry_to_geo(&c))
                .collect::<Result<Vec<_>>>()?,
        )),
        Rect(g) => geo::Geometry::Rect(g.to_rect()),
        Line(g) => geo::Geometry::Line(g.to_line()),
        Triangle(g) => geo::Geometry::Triangle(g.to_triangle()),
    })
}

pub fn simplify_geometry(geom: &geo::Geometry, epsilon: f64) -> geo::Geometry {
    use geo::Simplify as _;
    match geom {
        geo::Geometry::LineString(g) => geo::Geometry::LineString(g.simplify(epsilon)),
        geo::Geometry::Polygon(g) => geo::Geometry::Polygon(g.simplify(epsilon)),
        geo::Geometry::MultiLineString(g) => geo::Geometry::MultiLineString(g.simplify(epsilon)),
        geo::Geometry::MultiPolygon(g) => geo::Geometry::MultiPolygon(g.simplify(epsilon)),
        _ => geom.clone(),
    }
}

/// G3's `to_geos`: geo-traits straight to GEOS with `CoordSeq::new_from_buffer` (2D here).
pub fn to_geos(g: &impl GeometryTrait<T = f64>) -> Result<geos::Geometry> {
    fn seq(l: &impl LineStringTrait<T = f64>) -> Result<geos::CoordSeq> {
        let mut buf = Vec::with_capacity(l.num_coords() * 2);
        for c in l.coords() {
            buf.push(c.x());
            buf.push(c.y());
        }
        geos::CoordSeq::new_from_buffer(&buf, l.num_coords(), geos::CoordType::XY).map_err(ext)
    }
    fn poly(p: &impl PolygonTrait<T = f64>) -> Result<geos::Geometry> {
        let Some(ext_ring) = p.exterior() else {
            return geos::Geometry::create_empty_polygon().map_err(ext);
        };
        let shell = seq(&ext_ring)?.create_linear_ring().map_err(ext)?;
        let holes = p
            .interiors()
            .map(|r| seq(&r)?.create_linear_ring().map_err(ext))
            .collect::<Result<Vec<_>>>()?;
        geos::Geometry::create_polygon(shell, holes).map_err(ext)
    }
    match g.as_type() {
        GeometryType::Point(p) => match p.coord() {
            Some(c) => geos::CoordSeq::new_from_buffer(&[c.x(), c.y()], 1, geos::CoordType::XY)
                .and_then(|s| s.create_point())
                .map_err(ext),
            None => geos::Geometry::create_empty_point().map_err(ext),
        },
        GeometryType::LineString(l) => seq(l)?.create_line_string().map_err(ext),
        GeometryType::Polygon(p) => poly(p),
        GeometryType::MultiPolygon(m) => geos::Geometry::create_multipolygon(
            m.polygons().map(|p| poly(&p)).collect::<Result<Vec<_>>>()?,
        )
        .map_err(ext),
        _ => Err(DataFusionError::NotImplemented(
            "to_geos: type not needed by E1".to_string(),
        )),
    }
}
