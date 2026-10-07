//! Conversion between geo-traits geometries and GEOS geometries.
//!
//! GEOS-backed PostGIS functions keep Z and drop M (`LWGEOM2GEOS`). Converting through
//! geo-traits does the same, where a WKB round trip couldn't: GEOS >= 3.12 reads M from WKB,
//! and the geos crate's WKB writer writes 2D or extended WKB.

use datafusion::common::exec_datafusion_err;
use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    LineTrait, MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
    RectTrait, TriangleTrait,
};
use geos::{CoordSeq, CoordType, Geom, Geometry, GeometryTypes};
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::ordinates::z;

/// Converts a geometry to GEOS, keeping Z and dropping M, as PostGIS's `LWGEOM2GEOS` does.
///
/// The Z of the outer geometry applies to every part, so GEOS never sees mixed dimensions.
pub(crate) fn to_geos(geom: &impl GeometryTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
    Converter { has_z: has_z(geom) }.geometry(geom)
}

struct Converter {
    has_z: bool,
}

impl Converter {
    fn coord_seq<C: CoordTrait<T = f64>>(
        &self,
        coords: impl Iterator<Item = C>,
    ) -> GeoDataFusionResult<CoordSeq> {
        let mut buffer = vec![];
        let mut size = 0;
        for coord in coords {
            buffer.push(coord.x());
            buffer.push(coord.y());
            if self.has_z {
                buffer.push(z(&coord).unwrap_or(f64::NAN));
            }
            size += 1;
        }
        let coord_type = if self.has_z {
            CoordType::XYZ
        } else {
            CoordType::XY
        };
        Ok(CoordSeq::new_from_buffer(&buffer, size, coord_type)?)
    }

    fn point(&self, point: &impl PointTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
        match point.coord() {
            Some(coord) => Ok(Geometry::create_point(
                self.coord_seq(std::iter::once(coord))?,
            )?),
            None => Ok(Geometry::create_empty_point()?),
        }
    }

    fn line_string(&self, line: &impl LineStringTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
        if line.num_coords() == 0 {
            return Ok(Geometry::create_empty_line_string()?);
        }
        Ok(Geometry::create_line_string(
            self.coord_seq(line.coords())?,
        )?)
    }

    fn ring(&self, ring: &impl LineStringTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
        Ok(Geometry::create_linear_ring(
            self.coord_seq(ring.coords())?,
        )?)
    }

    fn polygon(&self, polygon: &impl PolygonTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
        let Some(exterior) = polygon.exterior().filter(|ring| ring.num_coords() > 0) else {
            return Ok(Geometry::create_empty_polygon()?);
        };
        let interiors = polygon
            .interiors()
            .map(|ring| self.ring(&ring))
            .collect::<GeoDataFusionResult<Vec<_>>>()?;
        Ok(Geometry::create_polygon(self.ring(&exterior)?, interiors)?)
    }

    fn geometry(&self, geom: &impl GeometryTrait<T = f64>) -> GeoDataFusionResult<Geometry> {
        Ok(match geom.as_type() {
            GeometryType::Point(point) => self.point(point)?,
            GeometryType::LineString(line) => self.line_string(line)?,
            GeometryType::Polygon(polygon) => self.polygon(polygon)?,
            GeometryType::MultiPoint(points) => Geometry::create_multipoint(
                points
                    .points()
                    .map(|point| self.point(&point))
                    .collect::<GeoDataFusionResult<_>>()?,
            )?,
            GeometryType::MultiLineString(lines) => Geometry::create_multiline_string(
                lines
                    .line_strings()
                    .map(|line| self.line_string(&line))
                    .collect::<GeoDataFusionResult<_>>()?,
            )?,
            GeometryType::MultiPolygon(polygons) => Geometry::create_multipolygon(
                polygons
                    .polygons()
                    .map(|polygon| self.polygon(&polygon))
                    .collect::<GeoDataFusionResult<_>>()?,
            )?,
            GeometryType::GeometryCollection(collection) => Geometry::create_geometry_collection(
                collection
                    .geometries()
                    .map(|member| self.geometry(&member))
                    .collect::<GeoDataFusionResult<_>>()?,
            )?,
            // As the polygon or linestring PostGIS would see.
            GeometryType::Rect(rect) => {
                let (min, max) = (rect.min(), rect.max());
                let ring = [
                    (min.x(), min.y()),
                    (max.x(), min.y()),
                    (max.x(), max.y()),
                    (min.x(), max.y()),
                    (min.x(), min.y()),
                ];
                let coords: Vec<f64> = ring.iter().flat_map(|&(x, y)| [x, y]).collect();
                let ring = CoordSeq::new_from_buffer(&coords, ring.len(), CoordType::XY)?;
                Geometry::create_polygon(Geometry::create_linear_ring(ring)?, vec![])?
            }
            GeometryType::Triangle(triangle) => {
                let coords: Vec<Coord<f64>> = triangle
                    .coords()
                    .iter()
                    .map(|c| Coord {
                        x: c.x(),
                        y: c.y(),
                        z: z(c),
                        m: None,
                    })
                    .collect();
                let ring = self.coord_seq(coords.iter().chain(coords.first()).cloned())?;
                Geometry::create_polygon(Geometry::create_linear_ring(ring)?, vec![])?
            }
            GeometryType::Line(line) => Geometry::create_line_string(
                self.coord_seq([line.start(), line.end()].into_iter())?,
            )?,
        })
    }
}

/// An EMPTY geometry of the same type and dimension as `geom`, M included.
///
/// Many GEOS-backed PostGIS functions return EMPTY input unchanged, where GEOS would return
/// another type or drop M.
pub(crate) fn empty_like(geom: &impl GeometryTrait<T = f64>) -> Wkt<f64> {
    let dim = dimension(geom);
    match geom.as_type() {
        GeometryType::Point(_) => Wkt::Point(Point::empty(dim)),
        GeometryType::LineString(_) | GeometryType::Line(_) => {
            Wkt::LineString(LineString::empty(dim))
        }
        GeometryType::Polygon(_) | GeometryType::Rect(_) | GeometryType::Triangle(_) => {
            Wkt::Polygon(Polygon::empty(dim))
        }
        GeometryType::MultiPoint(_) => Wkt::MultiPoint(MultiPoint::empty(dim)),
        GeometryType::MultiLineString(_) => Wkt::MultiLineString(MultiLineString::empty(dim)),
        GeometryType::MultiPolygon(_) => Wkt::MultiPolygon(MultiPolygon::empty(dim)),
        GeometryType::GeometryCollection(_) => {
            Wkt::GeometryCollection(GeometryCollection::empty(dim))
        }
    }
}

/// Whether a geometry has Z.
pub(crate) fn has_z(geom: &impl GeometryTrait<T = f64>) -> bool {
    matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm)
}

/// An EMPTY point with the dimension of `geom`, M included, which several GEOS-backed PostGIS
/// functions return for EMPTY input.
pub(crate) fn empty_point_like(geom: &impl GeometryTrait<T = f64>) -> Wkt<f64> {
    Wkt::Point(Point::empty(dimension(geom)))
}

fn dimension(geom: &impl GeometryTrait<T = f64>) -> Dimension {
    match geom.dim() {
        Dimensions::Xyz => Dimension::XYZ,
        Dimensions::Xym => Dimension::XYM,
        Dimensions::Xyzm => Dimension::XYZM,
        _ => Dimension::XY,
    }
}

/// Converts a GEOS geometry to an owned geo-traits geometry.
///
/// The result has Z only when GEOS's does and `want_z` is set, as PostGIS's `GEOS2LWGEOM` does:
/// a GEOS operation can return Z for 2D input (an EMPTY result, for one), and PostGIS sets
/// `want_z` when an input has Z.
pub(crate) fn from_geos(geom: &impl Geom, want_z: bool) -> GeoDataFusionResult<Wkt<f64>> {
    let dim = if want_z && geom.has_z()? {
        Dimension::XYZ
    } else {
        Dimension::XY
    };
    Ok(match geom.geometry_type()? {
        GeometryTypes::Point => Wkt::Point(point_from_geos(geom, dim)?),
        GeometryTypes::LineString | GeometryTypes::LinearRing => {
            Wkt::LineString(line_string_from_geos(geom, dim)?)
        }
        GeometryTypes::Polygon => Wkt::Polygon(polygon_from_geos(geom, dim)?),
        GeometryTypes::MultiPoint => Wkt::MultiPoint(MultiPoint::new(
            members(geom, want_z, |member| point_from_geos(member, dim))?,
            dim,
        )),
        GeometryTypes::MultiLineString => Wkt::MultiLineString(MultiLineString::new(
            members(geom, want_z, |member| line_string_from_geos(member, dim))?,
            dim,
        )),
        GeometryTypes::MultiPolygon => Wkt::MultiPolygon(MultiPolygon::new(
            members(geom, want_z, |member| polygon_from_geos(member, dim))?,
            dim,
        )),
        GeometryTypes::GeometryCollection => Wkt::GeometryCollection(GeometryCollection::new(
            members(geom, want_z, |member| from_geos(member, want_z))?,
            dim,
        )),
    })
}

/// The members of a GEOS collection, converted. When Z is wanted, members with and without Z
/// are an error, as in PostGIS, which can't represent them in one collection.
fn members<T>(
    geom: &impl Geom,
    want_z: bool,
    convert: impl Fn(&geos::ConstGeometry<'_>) -> GeoDataFusionResult<T>,
) -> GeoDataFusionResult<Vec<T>> {
    let has_z = geom.has_z()?;
    (0..geom.get_num_geometries()?)
        .map(|n| {
            let member = geom.get_geometry_n(n)?;
            if want_z && member.has_z()? != has_z {
                return Err(
                    exec_datafusion_err!("mixed dimension geometries in a GEOS result").into(),
                );
            }
            convert(&member)
        })
        .collect()
}

fn coords_from_geos(geom: &impl Geom, dim: Dimension) -> GeoDataFusionResult<Vec<Coord<f64>>> {
    let seq = geom.get_coord_seq()?;
    (0..seq.size()?)
        .map(|i| {
            Ok(Coord {
                x: seq.get_x(i)?,
                y: seq.get_y(i)?,
                z: match dim {
                    Dimension::XYZ => Some(seq.get_z(i)?),
                    _ => None,
                },
                m: None,
            })
        })
        .collect()
}

fn point_from_geos(geom: &impl Geom, dim: Dimension) -> GeoDataFusionResult<Point<f64>> {
    if geom.is_empty()? {
        return Ok(Point::empty(dim));
    }
    let coord = coords_from_geos(geom, dim)?.into_iter().next();
    Ok(Point::new(coord, dim))
}

fn line_string_from_geos(geom: &impl Geom, dim: Dimension) -> GeoDataFusionResult<LineString<f64>> {
    if geom.is_empty()? {
        return Ok(LineString::empty(dim));
    }
    Ok(LineString::new(coords_from_geos(geom, dim)?, dim))
}

fn polygon_from_geos(geom: &impl Geom, dim: Dimension) -> GeoDataFusionResult<Polygon<f64>> {
    if geom.is_empty()? {
        return Ok(Polygon::empty(dim));
    }
    let mut rings = vec![line_string_from_geos(&geom.get_exterior_ring()?, dim)?];
    for n in 0..geom.get_num_interior_rings()? {
        rings.push(line_string_from_geos(&geom.get_interior_ring_n(n)?, dim)?);
    }
    Ok(Polygon::new(rings, dim))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::udf::native::io::util::wkt::{WktFlavor, parse_ewkt, write_wkt};

    fn round_trip(wkt: &str) -> String {
        let (_, geom) = parse_ewkt(wkt).unwrap();
        let mut out = String::new();
        write_wkt(
            &mut out,
            &from_geos(&to_geos(&geom).unwrap(), has_z(&geom)).unwrap(),
            WktFlavor::Iso,
            15,
        );
        out
    }

    #[test]
    fn test_round_trip_keeps_z_and_drops_m() {
        for (input, expected) in [
            ("POINT(1 2)", "POINT(1 2)"),
            ("POINT Z (1 2 3)", "POINT Z (1 2 3)"),
            ("POINT M (1 2 3)", "POINT(1 2)"),
            (
                "LINESTRING ZM (0 0 1 5,1 1 2 6)",
                "LINESTRING Z (0 0 1,1 1 2)",
            ),
            (
                "POLYGON((0 0,10 0,10 10,0 0),(1 1,2 1,2 2,1 1))",
                "POLYGON((0 0,10 0,10 10,0 0),(1 1,2 1,2 2,1 1))",
            ),
            ("MULTIPOINT((1 2),(3 4))", "MULTIPOINT((1 2),(3 4))"),
            (
                "MULTILINESTRING Z ((0 0 1,1 1 2))",
                "MULTILINESTRING Z ((0 0 1,1 1 2))",
            ),
            (
                "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))",
                "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))",
            ),
            (
                "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))",
                "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1))",
            ),
            ("POINT EMPTY", "POINT EMPTY"),
            ("LINESTRING EMPTY", "LINESTRING EMPTY"),
            ("POLYGON EMPTY", "POLYGON EMPTY"),
            ("GEOMETRYCOLLECTION EMPTY", "GEOMETRYCOLLECTION EMPTY"),
        ] {
            assert_eq!(round_trip(input), expected, "{input}");
        }
    }
}
