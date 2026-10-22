//! Planar distances between geometries, and the points they are measured between: the nearest
//! pair (ST_Distance, ST_DWithin, ST_ClosestPoint, ST_ShortestLine) and the farthest pair
//! (ST_MaxDistance, ST_LongestLine).
//!
//! Of pairs at the same distance, the first one found wins, going through the parts of the first
//! geometry, then those of the second, in order.

use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};

/// A point in the plane.
pub(crate) type Xy = (f64, f64);

/// A distance and the points it is measured between, the first on the first geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measure {
    pub(crate) distance: f64,
    pub(crate) points: [Xy; 2],
}

impl Measure {
    /// The measure the other way round.
    fn swap(self) -> Self {
        let [p, q] = self.points;
        Measure {
            distance: self.distance,
            points: [q, p],
        }
    }

    /// The nearer of two measures; the first if they are equally near.
    fn nearer(self, other: Measure) -> Measure {
        if other.distance < self.distance {
            other
        } else {
            self
        }
    }

    /// The farther of two measures; the first if they are equally far.
    fn farther(self, other: Measure) -> Measure {
        if other.distance > self.distance {
            other
        } else {
            self
        }
    }
}

/// The minimum planar distance between two geometries, or `None` if either is empty.
pub(crate) fn distance(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<f64> {
    nearest(geom1, geom2).map(|measure| measure.distance)
}

/// The minimum planar distance between two geometries and the nearest points, or `None` if
/// either is empty.
pub(crate) fn nearest(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<Measure> {
    let (parts1, parts2) = (parts(geom1), parts(geom2));
    if parts1.is_empty() || parts2.is_empty() {
        return None;
    }
    let boxes1: Vec<Bbox> = parts1.iter().map(Part::bbox).collect();
    let boxes2: Vec<Bbox> = parts2.iter().map(Part::bbox).collect();
    let mut best: Option<Measure> = None;
    for (part1, box1) in parts1.iter().zip(&boxes1) {
        for (part2, box2) in parts2.iter().zip(&boxes2) {
            // The gap between the boxes is a lower bound, so this pair can't be nearer.
            if best.is_some_and(|best| box1.gap(box2) > best.distance) {
                continue;
            }
            let measure = part_nearest(part1, part2);
            let best = best.insert(best.map_or(measure, |best| best.nearer(measure)));
            if best.distance == 0.0 {
                return Some(*best);
            }
        }
    }
    best
}

/// The maximum planar distance between two geometries and the farthest points, or `None` if
/// either is empty.
///
/// The farthest points are always vertices. PostGIS looks at a polygon's shell alone, but at
/// every ring of the polygons in a collection, so a hole sticking out of an invalid polygon
/// counts there.
pub(crate) fn farthest(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<Measure> {
    let (vertices1, vertices2) = (farthest_vertices(geom1), farthest_vertices(geom2));
    let mut best: Option<Measure> = None;
    for part1 in &vertices1 {
        for part2 in &vertices2 {
            for &p in part1 {
                for &q in part2 {
                    let measure = Measure {
                        distance: point_point(p, q),
                        points: [p, q],
                    };
                    best = Some(best.map_or(measure, |best| best.farther(measure)));
                }
            }
        }
    }
    best
}

/// The vertices [`farthest`] looks at, part by part.
fn farthest_vertices(geom: &impl GeometryTrait<T = f64>) -> Vec<Vec<Xy>> {
    let lone_polygon = matches!(geom.as_type(), GeometryType::Polygon(_));
    parts(geom)
        .into_iter()
        .map(|part| match part {
            Part::Polygon(rings) if !lone_polygon => rings.into_iter().flatten().collect(),
            part => part.outline().to_vec(),
        })
        .collect()
}

/// A bounding box.
struct Bbox {
    min: Xy,
    max: Xy,
}

impl Bbox {
    fn of(coords: impl IntoIterator<Item = Xy>) -> Self {
        let empty = Bbox {
            min: (f64::INFINITY, f64::INFINITY),
            max: (f64::NEG_INFINITY, f64::NEG_INFINITY),
        };
        coords.into_iter().fold(empty, |bbox, (x, y)| Bbox {
            min: (bbox.min.0.min(x), bbox.min.1.min(y)),
            max: (bbox.max.0.max(x), bbox.max.1.max(y)),
        })
    }

    /// The distance between the boxes; 0 if they intersect.
    fn gap(&self, other: &Bbox) -> f64 {
        let dx = (other.min.0 - self.max.0)
            .max(self.min.0 - other.max.0)
            .max(0.0);
        let dy = (other.min.1 - self.max.1)
            .max(self.min.1 - other.max.1)
            .max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// A non-empty basic part of a geometry. A polygon's first ring is its shell.
enum Part {
    Point(Xy),
    Line(Vec<Xy>),
    Polygon(Vec<Vec<Xy>>),
}

impl Part {
    fn bbox(&self) -> Bbox {
        Bbox::of(self.outline().iter().copied())
    }

    /// The point, the line's points, or the polygon's shell: the holes are inside it.
    fn outline(&self) -> &[Xy] {
        match self {
            Part::Point(p) => std::slice::from_ref(p),
            Part::Line(line) => line,
            Part::Polygon(rings) => &rings[0],
        }
    }
}

/// The non-empty points, lines and polygons of a geometry, collections flattened.
fn parts(geom: &impl GeometryTrait<T = f64>) -> Vec<Part> {
    let mut parts = Vec::new();
    push_parts(geom, &mut parts);
    parts
}

fn push_parts(geom: &impl GeometryTrait<T = f64>, parts: &mut Vec<Part>) {
    match geom.as_type() {
        GeometryType::Point(point) => parts.extend(point_part(point)),
        GeometryType::LineString(line) => parts.extend(line_part(line)),
        GeometryType::Polygon(polygon) => parts.extend(polygon_part(polygon)),
        GeometryType::MultiPoint(points) => {
            parts.extend(points.points().filter_map(|point| point_part(&point)));
        }
        GeometryType::MultiLineString(lines) => {
            parts.extend(lines.line_strings().filter_map(|line| line_part(&line)));
        }
        GeometryType::MultiPolygon(polygons) => {
            parts.extend(
                polygons
                    .polygons()
                    .filter_map(|polygon| polygon_part(&polygon)),
            );
        }
        GeometryType::GeometryCollection(collection) => {
            for member in collection.geometries() {
                push_parts(&member, parts);
            }
        }
        GeometryType::Rect(rect) => {
            let (min, max) = (xy(&rect.min()), xy(&rect.max()));
            let ring = vec![min, (max.0, min.1), max, (min.0, max.1), min];
            parts.push(Part::Polygon(vec![ring]));
        }
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| xy(&coord));
            parts.push(Part::Polygon(vec![vec![a, b, c, a]]));
        }
        GeometryType::Line(line) => {
            parts.push(Part::Line(vec![xy(&line.start()), xy(&line.end())]));
        }
    }
}

fn xy(coord: &impl CoordTrait<T = f64>) -> Xy {
    (coord.x(), coord.y())
}

fn point_part(point: &impl PointTrait<T = f64>) -> Option<Part> {
    point.coord().map(|coord| Part::Point(xy(&coord)))
}

fn line_part(line: &impl LineStringTrait<T = f64>) -> Option<Part> {
    let coords: Vec<Xy> = line.coords().map(|coord| xy(&coord)).collect();
    (!coords.is_empty()).then_some(Part::Line(coords))
}

fn polygon_part(polygon: &impl PolygonTrait<T = f64>) -> Option<Part> {
    let rings: Vec<Vec<Xy>> = polygon
        .exterior()
        .into_iter()
        .chain(polygon.interiors())
        .map(|ring| ring.coords().map(|coord| xy(&coord)).collect::<Vec<_>>())
        .collect();
    (!rings.first()?.is_empty()).then_some(Part::Polygon(rings))
}

/// A measure of 0 at `p`, for a point of one geometry inside the other.
fn inside(p: Xy) -> Measure {
    Measure {
        distance: 0.0,
        points: [p, p],
    }
}

fn part_nearest(part1: &Part, part2: &Part) -> Measure {
    match (part1, part2) {
        (Part::Point(p), Part::Point(q)) => Measure {
            distance: point_point(*p, *q),
            points: [*p, *q],
        },
        (Part::Point(p), Part::Line(line)) => point_line(*p, line),
        (Part::Line(line), Part::Point(p)) => point_line(*p, line).swap(),
        (Part::Line(line1), Part::Line(line2)) => line_line(line1, line2),
        (Part::Point(p), Part::Polygon(rings)) => match facing_ring(*p, rings) {
            None => inside(*p),
            Some(ring) => point_line(*p, ring),
        },
        (Part::Polygon(rings), Part::Point(p)) => match facing_ring(*p, rings) {
            None => inside(*p),
            Some(ring) => point_line(*p, ring).swap(),
        },
        // A line that doesn't cross the ring it starts behind stays behind it.
        (Part::Line(line), Part::Polygon(rings)) => match facing_ring(line[0], rings) {
            None => line_inside(line, rings),
            Some(ring) => line_line(line, ring),
        },
        (Part::Polygon(rings), Part::Line(line)) => match facing_ring(line[0], rings) {
            None => line_inside(line, rings).swap(),
            Some(ring) => line_line(line, ring).swap(),
        },
        // Likewise for the polygons' shells.
        (Part::Polygon(rings1), Part::Polygon(rings2)) => {
            let (start1, start2) = (rings1[0][0], rings2[0][0]);
            match (facing_ring(start1, rings2), facing_ring(start2, rings1)) {
                (None, _) => inside(start1),
                (_, None) => inside(start2),
                (Some(ring2), Some(ring1)) => line_line(ring1, ring2),
            }
        }
    }
}

/// The measure of 0 for a line that starts inside a polygon. PostGIS still measures the line to
/// the holes, so where it first meets one is where it reports, otherwise the line's start.
fn line_inside(line: &[Xy], rings: &[Vec<Xy>]) -> Measure {
    rings[1..]
        .iter()
        .map(|hole| line_line(line, hole))
        .find(|measure| measure.distance == 0.0)
        .unwrap_or_else(|| inside(line[0]))
}

/// The ring of a polygon that separates `p` from the polygon's interior: the shell if `p` is
/// outside it, or the hole `p` is in. `None` if `p` is inside the polygon.
///
/// The distance from outside a polygon is the distance to that ring alone, as in PostGIS. For
/// a valid polygon the other rings are farther; for an invalid one, with a hole sticking out
/// of the shell, PostGIS ignores them all the same.
fn facing_ring(p: Xy, rings: &[Vec<Xy>]) -> Option<&[Xy]> {
    let (shell, holes) = rings.split_first().expect("a polygon part has a shell");
    if !in_ring(p, shell) {
        return Some(shell);
    }
    holes
        .iter()
        .find(|hole| in_ring(p, hole))
        .map(Vec::as_slice)
}

fn point_point(p: Xy, q: Xy) -> f64 {
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    (dx * dx + dy * dy).sqrt()
}

/// The measure from `p` to the nearest point of the line.
fn point_line(p: Xy, line: &[Xy]) -> Measure {
    match line {
        [only] => Measure {
            distance: point_point(p, *only),
            points: [p, *only],
        },
        _ => line
            .windows(2)
            .map(|segment| point_segment(p, segment[0], segment[1]))
            .reduce(Measure::nearer)
            .expect("a line has a segment"),
    }
}

fn line_line(line1: &[Xy], line2: &[Xy]) -> Measure {
    match (line1, line2) {
        ([p], line) => point_line(*p, line),
        (line, [p]) => point_line(*p, line).swap(),
        _ => {
            let mut best: Option<Measure> = None;
            for a in line1.windows(2) {
                let box_a = Bbox::of([a[0], a[1]]);
                for b in line2.windows(2) {
                    if best.is_some_and(|best| box_a.gap(&Bbox::of([b[0], b[1]])) > best.distance) {
                        continue;
                    }
                    let measure = segment_segment(a[0], a[1], b[0], b[1]);
                    let best = best.insert(best.map_or(measure, |best| best.nearer(measure)));
                    if best.distance == 0.0 {
                        return *best;
                    }
                }
            }
            best.expect("both lines have segments")
        }
    }
}

/// The measure from `p` to the segment `a`-`b`: to the nearest end if `p` projects outside
/// the segment, otherwise to its projection.
///
/// The distance from the segment's line is computed as JTS does (`Distance.pointToSegment`), from
/// the signed area `p` spans with the segment. It agrees with PostGIS to the last digit, also for
/// points a rounding error away from the line, where the distance to the projection doesn't.
fn point_segment(p: Xy, a: Xy, b: Xy) -> Measure {
    let to = |q: Xy| Measure {
        distance: point_point(p, q),
        points: [p, q],
    };
    if a == b {
        return to(a);
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let r = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_squared;
    if r <= 0.0 {
        return to(a);
    }
    if r >= 1.0 {
        return to(b);
    }
    let s = ((a.1 - p.1) * dx - (a.0 - p.0) * dy) / length_squared;
    Measure {
        distance: s.abs() * length_squared.sqrt(),
        points: [p, (a.0 + r * dx, a.1 + r * dy)],
    }
}

/// The measure between the segments `a`-`b` and `c`-`d`: 0 at their intersection if they
/// cross, otherwise the nearest of the ends of each to the other.
fn segment_segment(a: Xy, b: Xy, c: Xy, d: Xy) -> Measure {
    let (side_a, side_b) = (orientation(c, d, a), orientation(c, d, b));
    let (side_c, side_d) = (orientation(a, b, c), orientation(a, b, d));
    if side_a * side_b < 0.0 && side_c * side_d < 0.0 {
        // The intersection is where PostGIS's results put it: along `a`-`b` by the fraction
        // the comp.graphics.algorithms FAQ (1.03) gives.
        let r = ((a.1 - c.1) * (d.0 - c.0) - (a.0 - c.0) * (d.1 - c.1))
            / ((b.0 - a.0) * (d.1 - c.1) - (b.1 - a.1) * (d.0 - c.0));
        return inside((a.0 + r * (b.0 - a.0), a.1 + r * (b.1 - a.1)));
    }
    point_segment(a, c, d)
        .nearer(point_segment(b, c, d))
        .nearer(point_segment(c, a, b).swap())
        .nearer(point_segment(d, a, b).swap())
}

/// Positive if `p` is left of the line from `a` to `b`, negative if right, 0 if on it.
fn orientation(a: Xy, b: Xy, p: Xy) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// The winding number rule: whether the ring winds around `p`. Which side of an edge `p` is on
/// comes from the sign of `orientation`, so points a rounding error from the ring are classified
/// as PostGIS classifies them.
fn in_ring(p: Xy, ring: &[Xy]) -> bool {
    let mut winding = 0;
    for segment in ring.windows(2) {
        let (a, b) = (segment[0], segment[1]);
        if a.1 <= p.1 {
            if b.1 > p.1 && orientation(a, b, p) > 0.0 {
                winding += 1;
            }
        } else if b.1 <= p.1 && orientation(a, b, p) < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}
