//! Whether one geography covers another, as PostGIS's ST_Covers(geography, geography) decides:
//! on the sphere, by the vertices and edges of the parts.
//!
//! A part of the first geometry must cover every part of the second, so a MULTIPOINT with a
//! point in each of two polygons isn't covered by their MULTIPOLYGON. Of single parts:
//!
//! - A point covers a point at the same coordinates.
//! - A line covers a point on one of its edges: within 5e-14 of its great circle's plane, on the
//!   unit sphere, and between its ends, or at the point of an edge of length zero. It covers a
//!   line whose first and last points it covers, whatever lies between them. PostGIS's test
//!   rounds the wrong way for about one edge in twenty and misses the edge's last point, so a
//!   line can fail to cover its own end, or itself; that isn't reproduced.
//! - A polygon covers a point in its shell, the shell's edges included, and in none of its
//!   holes, their edges included. It covers a line or polygon whose vertices it covers and
//!   whose edges don't cross or touch its rings, so not its own boundary, nor itself. A hole
//!   inside the covered polygon isn't noticed.
//! - Nothing else covers anything.

use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};

use crate::udf::native::util::geodetic_distance::{
    Spot, arc_normal, crossing, dot, in_ring, norm, normalize, on_arc, spot,
};

/// The distance from a great circle's plane within which a point is on it.
const TOLERANCE: f64 = 5e-14;

/// A part of a geometry: its points, as vertices.
enum Part {
    Point(Spot),
    Line(Vec<Spot>),
    /// The rings, the shell first.
    Polygon(Vec<Vec<Spot>>),
}

/// Whether `a` covers `b`; false if either is EMPTY.
#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_Covers and ST_CoveredBy, backed by GEOS, take geography"
    )
)]
pub(crate) fn covers(a: &impl GeometryTrait<T = f64>, b: &impl GeometryTrait<T = f64>) -> bool {
    let (a, b) = (parts(a), parts(b));
    !b.is_empty() && a.iter().any(|a| b.iter().all(|b| part_covers(a, b)))
}

fn parts(geom: &impl GeometryTrait<T = f64>) -> Vec<Part> {
    let mut parts = Vec::new();
    add_parts(geom, &mut parts);
    parts
}

fn add_parts(geom: &impl GeometryTrait<T = f64>, parts: &mut Vec<Part>) {
    match geom.as_type() {
        GeometryType::Point(point) => parts.extend(point.coord().map(|c| Part::Point(spot(&c)))),
        GeometryType::MultiPoint(points) => {
            for point in points.points() {
                parts.extend(point.coord().map(|c| Part::Point(spot(&c))));
            }
        }
        GeometryType::LineString(line) => add_line(line, parts),
        GeometryType::MultiLineString(lines) => {
            for line in lines.line_strings() {
                add_line(&line, parts);
            }
        }
        GeometryType::Polygon(polygon) => add_polygon(polygon, parts),
        GeometryType::MultiPolygon(polygons) => {
            for polygon in polygons.polygons() {
                add_polygon(&polygon, parts);
            }
        }
        GeometryType::GeometryCollection(collection) => {
            for member in collection.geometries() {
                add_parts(&member, parts);
            }
        }
        GeometryType::Line(line) => {
            parts.push(Part::Line(vec![spot(&line.start()), spot(&line.end())]))
        }
        GeometryType::Rect(rect) => {
            let (min, max) = (rect.min(), rect.max());
            let ring = [
                (min.x(), min.y()),
                (min.x(), max.y()),
                (max.x(), max.y()),
                (max.x(), min.y()),
                (min.x(), min.y()),
            ]
            .map(|(x, y)| Spot::from_degrees(x, y))
            .to_vec();
            parts.push(Part::Polygon(vec![ring]));
        }
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| spot(&coord));
            parts.push(Part::Polygon(vec![vec![a, b, c, a]]));
        }
    }
}

fn add_line(line: &impl LineStringTrait<T = f64>, parts: &mut Vec<Part>) {
    let spots: Vec<Spot> = line.coords().map(|coord| spot(&coord)).collect();
    if !spots.is_empty() {
        parts.push(Part::Line(spots));
    }
}

fn add_polygon(polygon: &impl PolygonTrait<T = f64>, parts: &mut Vec<Part>) {
    let rings: Vec<Vec<Spot>> = polygon
        .exterior()
        .into_iter()
        .chain(polygon.interiors())
        .map(|ring| ring.coords().map(|coord| spot(&coord)).collect::<Vec<_>>())
        .filter(|ring| !ring.is_empty())
        .collect();
    if !rings.is_empty() {
        parts.push(Part::Polygon(rings));
    }
}

fn part_covers(a: &Part, b: &Part) -> bool {
    match (a, b) {
        (Part::Point(p), Part::Point(q)) => p.lon == q.lon && p.lat == q.lat,
        (Part::Line(line), Part::Point(q)) => on_line(line, q),
        (Part::Line(line), Part::Line(other)) => [other.first(), other.last()]
            .into_iter()
            .flatten()
            .all(|q| on_line(line, q)),
        (Part::Polygon(rings), Part::Point(q)) => in_polygon(rings, q),
        (Part::Polygon(rings), Part::Line(line)) => polygon_covers_rings(rings, [line]),
        (Part::Polygon(rings), Part::Polygon(other)) => polygon_covers_rings(rings, other),
        _ => false,
    }
}

/// Whether a polygon covers the vertices of `lines` and no edge of theirs crosses or touches
/// its rings.
fn polygon_covers_rings<'a>(
    rings: &[Vec<Spot>],
    lines: impl IntoIterator<Item = &'a Vec<Spot>> + Clone,
) -> bool {
    lines
        .clone()
        .into_iter()
        .all(|line| line.iter().all(|q| in_polygon(rings, q)))
        && !lines.into_iter().any(|line| {
            edges(line).any(|edge| {
                rings
                    .iter()
                    .any(|ring| edges(ring).any(|e| meet(&edge, &e)))
            })
        })
}

fn in_polygon(rings: &[Vec<Spot>], q: &Spot) -> bool {
    let in_or_on = |ring: &Vec<Spot>| on_line(ring, q) || in_ring(ring, q);
    let (shell, holes) = rings.split_first().expect("a polygon part has a shell");
    in_or_on(shell) && !holes.iter().any(in_or_on)
}

/// Whether `q` is on an edge of `line`.
fn on_line(line: &[Spot], q: &Spot) -> bool {
    edges(line).any(|edge| on_edge(&edge, q))
}

fn on_edge(edge: &[Spot; 2], q: &Spot) -> bool {
    if edge[0].vector() == edge[1].vector() {
        return q.vector() == edge[0].vector();
    }
    let normal = arc_normal(&edge[0], &edge[1]);
    let length = norm(&normal);
    if length == 0.0 {
        return false;
    }
    let normal = normalize(&normal, length);
    dot(&normal, &q.vector()).abs() <= TOLERANCE && on_arc(&q.vector(), &edge[0], &edge[1], &normal)
}

/// Whether two edges cross or touch.
fn meet(e: &[Spot; 2], f: &[Spot; 2]) -> bool {
    crossing(e, f).is_some() || e.iter().any(|q| on_edge(f, q)) || f.iter().any(|q| on_edge(e, q))
}

fn edges(line: &[Spot]) -> impl Iterator<Item = [Spot; 2]> + '_ {
    line.windows(2).map(|pair| [pair[0], pair[1]])
}
