//! Locating the parts of a geometry by M or Z: at a measure (ST_LocateAlong) or within a range
//! (ST_LocateBetween, ST_LocateBetweenElevations).

use datafusion::common::exec_datafusion_err;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;

/// The points of a geometry with M at `measure`, as ST_LocateAlong returns them: a MULTIPOINT.
/// Polygons and collections are errors, named after the function `name`.
pub(crate) fn locate_along(
    name: &str,
    geom: Wkt<f64>,
    measure: f64,
    offset: f64,
) -> GeoDataFusionResult<Wkt<f64>> {
    let dim = match geom.dimension() {
        Dimension::XYZM => Dimension::XYZM,
        _ => Dimension::XYM,
    };
    let coords = match geom {
        Wkt::Point(point) => point
            .coord()
            .filter(|c| c.m == Some(measure))
            .copied()
            .into_iter()
            .collect(),
        Wkt::MultiPoint(points) => points
            .points()
            .iter()
            .filter_map(|point| point.coord().filter(|c| c.m == Some(measure)).copied())
            .collect(),
        Wkt::LineString(line) => along(line.coords(), measure, offset),
        Wkt::MultiLineString(lines) => lines
            .line_strings()
            .iter()
            .flat_map(|line| along(line.coords(), measure, offset))
            .collect(),
        Wkt::Polygon(_) | Wkt::MultiPolygon(_) | Wkt::GeometryCollection(_) => {
            let type_name = match geom {
                Wkt::Polygon(_) => "Polygon",
                Wkt::MultiPolygon(_) => "MultiPolygon",
                _ => "GeometryCollection",
            };
            return Err(exec_datafusion_err!(
                "{name}: Only linear geometries are supported, {type_name} provided."
            )
            .into());
        }
    };
    let points = coords
        .into_iter()
        .map(|coord| Point::new(Some(coord), dim))
        .collect();
    Ok(Wkt::MultiPoint(MultiPoint::new(points, dim)))
}

/// The points of a line at `measure`, each segment's in turn, without consecutive repeats.
fn along(coords: &[Coord<f64>], measure: f64, offset: f64) -> Vec<Coord<f64>> {
    let mut out: Vec<Coord<f64>> = vec![];
    for pair in coords.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (Some(ma), Some(mb)) = (a.m, b.m) else {
            continue;
        };
        if measure < ma.min(mb) || measure > ma.max(mb) {
            continue;
        }
        // A segment at the measure all along gives its midpoint.
        let t = if ma == mb {
            0.5
        } else {
            (measure - ma) / (mb - ma)
        };
        let mix = |a: f64, b: f64| a + (b - a) * t;
        let mut point = Coord {
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            z: a.z.zip(b.z).map(|(a, b)| mix(a, b)),
            m: Some(measure),
        };
        if offset != 0.0 {
            let theta = (b.y - a.y).atan2(b.x - a.x);
            point.x -= offset * theta.sin();
            point.y += offset * theta.cos();
        }
        if out.last() != Some(&point) {
            out.push(point);
        }
    }
    out
}

/// The ordinate to clip by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ordinate {
    Z,
    #[cfg_attr(
        not(feature = "geos-3_11"),
        expect(
            dead_code,
            reason = "only ST_LocateBetween, which offsets with GEOS, locates by M"
        )
    )]
    M,
}

impl Ordinate {
    fn get(self, c: &Coord<f64>) -> f64 {
        match self {
            Ordinate::Z => c.z,
            Ordinate::M => c.m,
        }
        .unwrap_or(f64::NAN)
    }
}

/// A part of a line within the range: a line, or a lone point where the line only touches it.
enum Piece {
    Point(Coord<f64>),
    Line(Vec<Coord<f64>>),
}

/// The parts of a geometry whose `ordinate` is between `from` and `to` (inclusive, either way
/// round), typed as PostGIS types them: MULTIPOINT for points, MULTILINESTRING for lines (a
/// GEOMETRYCOLLECTION if a line only touches the range somewhere, which gives a point),
/// MULTIPOLYGON for polygons, and a GEOMETRYCOLLECTION of the members' parts for a collection.
pub(crate) fn locate_between(geom: &Wkt<f64>, ordinate: Ordinate, from: f64, to: f64) -> Wkt<f64> {
    let range = Range {
        lo: from.min(to),
        hi: from.max(to),
        ordinate,
    };
    let dim = geom.dimension();
    match geom {
        Wkt::Point(_) | Wkt::MultiPoint(_) => {
            let points = range.points(geom);
            Wkt::MultiPoint(MultiPoint::new(points, dim))
        }
        Wkt::LineString(line) => range.pieces_geometry(range.line(line.coords()), dim),
        Wkt::MultiLineString(lines) => {
            let pieces = lines
                .line_strings()
                .iter()
                .flat_map(|line| range.line(line.coords()))
                .collect();
            range.pieces_geometry(pieces, dim)
        }
        Wkt::Polygon(polygon) => Wkt::MultiPolygon(MultiPolygon::new(
            range.polygon(polygon).into_iter().collect(),
            dim,
        )),
        Wkt::MultiPolygon(polygons) => Wkt::MultiPolygon(MultiPolygon::new(
            polygons
                .polygons()
                .iter()
                .filter_map(|polygon| range.polygon(polygon))
                .collect(),
            dim,
        )),
        Wkt::GeometryCollection(collection) => {
            let mut members = vec![];
            for member in collection.geometries() {
                match member {
                    Wkt::Polygon(polygon) => {
                        members.extend(range.polygon(polygon).map(Wkt::Polygon));
                    }
                    Wkt::MultiPolygon(polygons) => members.extend(
                        polygons
                            .polygons()
                            .iter()
                            .filter_map(|polygon| range.polygon(polygon))
                            .map(Wkt::Polygon),
                    ),
                    Wkt::LineString(line) => {
                        members.extend(range.piece_geometries(range.line(line.coords()), dim));
                    }
                    Wkt::MultiLineString(lines) => {
                        for line in lines.line_strings() {
                            members.extend(range.piece_geometries(range.line(line.coords()), dim));
                        }
                    }
                    Wkt::Point(_) | Wkt::MultiPoint(_) => {
                        members.extend(range.points(member).into_iter().map(Wkt::Point))
                    }
                    Wkt::GeometryCollection(_) => {
                        if let Wkt::GeometryCollection(nested) =
                            locate_between(member, ordinate, from, to)
                        {
                            members.extend(nested.geometries().iter().cloned());
                        }
                    }
                }
            }
            Wkt::GeometryCollection(GeometryCollection::new(members, dim))
        }
    }
}

struct Range {
    lo: f64,
    hi: f64,
    ordinate: Ordinate,
}

impl Range {
    fn inside(&self, c: &Coord<f64>) -> bool {
        let value = self.ordinate.get(c);
        value >= self.lo && value <= self.hi
    }

    /// The points of a point or multipoint within the range.
    fn points(&self, geom: &Wkt<f64>) -> Vec<Point<f64>> {
        let points: Vec<&Point<f64>> = match geom {
            Wkt::Point(point) => vec![point],
            Wkt::MultiPoint(points) => points.points().iter().collect(),
            _ => vec![],
        };
        points
            .into_iter()
            .filter(|point| point.coord().is_some_and(|c| self.inside(c)))
            .cloned()
            .collect()
    }

    /// The point of the segment from `from` towards `to` where the ordinate is `value`, with it
    /// set exactly. On lines, PostGIS's crossings sometimes differ from this in the last digit,
    /// in no pattern found; on polygon rings they agree.
    fn crossing(&self, from: &Coord<f64>, to: &Coord<f64>, value: f64) -> Coord<f64> {
        let (v_from, v_to) = (self.ordinate.get(from), self.ordinate.get(to));
        let f = (value - v_from) / (v_to - v_from);
        let mix = |a: f64, b: f64| a + (b - a) * f;
        let mut c = Coord {
            x: mix(from.x, to.x),
            y: mix(from.y, to.y),
            z: from.z.zip(to.z).map(|(a, b)| mix(a, b)),
            m: from.m.zip(to.m).map(|(a, b)| mix(a, b)),
        };
        match self.ordinate {
            Ordinate::Z => c.z = Some(value),
            Ordinate::M => c.m = Some(value),
        }
        c
    }

    /// The boundary a value outside the range is beyond.
    fn boundary(&self, value: f64) -> f64 {
        if value < self.lo { self.lo } else { self.hi }
    }

    /// The parts of a line within the range, in order.
    ///
    /// A vertex within the range belongs to the current part. Where a segment enters or leaves
    /// the range, the point on the boundary starts or ends a part, unless the vertex inside is on
    /// the boundary itself, so a line that only touches the range at a vertex gives a point. A
    /// segment crossing the whole range makes a part of its two crossings, even when they are
    /// the same point.
    fn line(&self, coords: &[Coord<f64>]) -> Vec<Piece> {
        let mut pieces = vec![];
        let mut current: Vec<Coord<f64>> = vec![];
        let mut finish = |current: &mut Vec<Coord<f64>>| match current.len() {
            0 => {}
            1 => pieces.push(Piece::Point(current.remove(0))),
            _ => pieces.push(Piece::Line(std::mem::take(current))),
        };
        for (index, c) in coords.iter().enumerate() {
            let inside = self.inside(c);
            if inside {
                current.push(*c);
            }
            let Some(next) = coords.get(index + 1) else {
                break;
            };
            let (value, next_value) = (self.ordinate.get(c), self.ordinate.get(next));
            match (inside, self.inside(next)) {
                (true, true) => {}
                (true, false) => {
                    let boundary = self.boundary(next_value);
                    if value != boundary {
                        current.push(self.crossing(c, next, boundary));
                    }
                    finish(&mut current);
                }
                (false, true) => {
                    let boundary = self.boundary(value);
                    if next_value != boundary {
                        current.push(self.crossing(c, next, boundary));
                    }
                }
                (false, false) => {
                    // A segment from below the range to above it, or back, crosses all of it.
                    if (value < self.lo && next_value > self.hi)
                        || (value > self.hi && next_value < self.lo)
                    {
                        current.push(self.crossing(c, next, self.boundary(value)));
                        current.push(self.crossing(c, next, self.boundary(next_value)));
                        finish(&mut current);
                    }
                }
            }
        }
        finish(&mut current);
        pieces
    }

    /// The parts of a polygon within the range: each ring's parts joined and closed. A ring left
    /// with fewer than four points goes, and the polygon with its shell.
    fn polygon(&self, polygon: &Polygon<f64>) -> Option<Polygon<f64>> {
        let dim = polygon.dimension();
        let mut rings = polygon.rings().iter().map(|ring| {
            let mut coords: Vec<Coord<f64>> = self
                .line(ring.coords())
                .into_iter()
                .flat_map(|piece| match piece {
                    Piece::Point(c) => vec![c],
                    Piece::Line(coords) => coords,
                })
                .collect();
            coords.dedup();
            if let Some(&first) = coords.first()
                && coords.last() != Some(&first)
            {
                coords.push(first);
            }
            (coords.len() >= 4).then(|| LineString::new(coords, dim))
        });
        let shell = rings.next().flatten()?;
        Some(Polygon::new(
            std::iter::once(shell).chain(rings.flatten()).collect(),
            dim,
        ))
    }

    /// The parts of lines as geometries.
    fn piece_geometries(&self, pieces: Vec<Piece>, dim: Dimension) -> Vec<Wkt<f64>> {
        pieces
            .into_iter()
            .map(|piece| match piece {
                Piece::Point(c) => Wkt::Point(Point::new(Some(c), dim)),
                Piece::Line(coords) => Wkt::LineString(LineString::new(coords, dim)),
            })
            .collect()
    }

    /// The parts of lines: a MULTILINESTRING if they are all lines, otherwise a
    /// GEOMETRYCOLLECTION.
    fn pieces_geometry(&self, pieces: Vec<Piece>, dim: Dimension) -> Wkt<f64> {
        if pieces.iter().all(|piece| matches!(piece, Piece::Line(_))) {
            let lines = pieces
                .into_iter()
                .filter_map(|piece| match piece {
                    Piece::Line(coords) => Some(LineString::new(coords, dim)),
                    Piece::Point(_) => None,
                })
                .collect();
            return Wkt::MultiLineString(MultiLineString::new(lines, dim));
        }
        Wkt::GeometryCollection(GeometryCollection::new(
            self.piece_geometries(pieces, dim),
            dim,
        ))
    }
}

/// Whether the geometry has the ordinate.
pub(crate) fn has_ordinate(geom: &impl GeometryTrait<T = f64>, ordinate: Ordinate) -> bool {
    use geo_traits::Dimensions;
    match ordinate {
        Ordinate::Z => matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm),
        Ordinate::M => matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm),
    }
}
