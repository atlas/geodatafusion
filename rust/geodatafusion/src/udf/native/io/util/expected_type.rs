//! The geometry type a type-checked constructor (ST_PointFromText, ST_PointFromWKB, ...) accepts.

use geo_traits::{GeometryTrait, GeometryType};

/// The geometry type a type-checked constructor accepts; it returns NULL for any other type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedType {
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
    GeometryCollection,
}

impl ExpectedType {
    /// Whether `geom` has this type. A collection doesn't match its members' type.
    pub(crate) fn matches(self, geom: &impl GeometryTrait<T = f64>) -> bool {
        matches!(
            (self, geom.as_type()),
            (ExpectedType::Point, GeometryType::Point(_))
                | (ExpectedType::LineString, GeometryType::LineString(_))
                | (ExpectedType::Polygon, GeometryType::Polygon(_))
                | (ExpectedType::MultiPoint, GeometryType::MultiPoint(_))
                | (
                    ExpectedType::MultiLineString,
                    GeometryType::MultiLineString(_)
                )
                | (ExpectedType::MultiPolygon, GeometryType::MultiPolygon(_))
                | (
                    ExpectedType::GeometryCollection,
                    GeometryType::GeometryCollection(_)
                )
        )
    }
}
