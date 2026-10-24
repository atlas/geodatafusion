//! Distances in 3D between geometries, and the points they are measured between: the nearest
//! pair (ST_3DDistance, ST_3DDWithin, ST_3DIntersects, ST_3DClosestPoint, ST_3DShortestLine) and
//! the farthest pair (ST_3DMaxDistance, ST_3DDFullyWithin, ST_3DLongestLine).
//!
//! A polygon is taken to lie in the plane through the average of its shell's vertices, with the
//! normal of Newell's method. A point that projects onto the plane inside the polygon is measured
//! to the plane, otherwise to the rings. PostGIS assumes planar polygons too, but fits the plane
//! of a non-planar one differently, so distances to those differ.
//!
//! Of pairs at the same distance, the first one found wins, going through the parts of the first
//! geometry, then those of the second, in order.

use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    LineTrait, MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
    RectTrait, TriangleTrait,
};

use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::util::distance;
use crate::udf::native::util::float_box::{round_down, round_up};

/// A point in space.
pub(crate) type Xyz = [f64; 3];

/// A distance and the points it is measured between, the first on the first geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measure3D {
    pub(crate) distance: f64,
    pub(crate) points: [Xyz; 2],
}

impl Measure3D {
    fn between(p: Xyz, q: Xyz) -> Self {
        Measure3D {
            distance: norm(sub(q, p)),
            points: [p, q],
        }
    }

    /// The measure the other way round.
    fn swap(self) -> Self {
        let [p, q] = self.points;
        Measure3D {
            distance: self.distance,
            points: [q, p],
        }
    }

    /// The nearer of two measures; the first if they are equally near.
    fn nearer(self, other: Measure3D) -> Measure3D {
        if other.distance < self.distance {
            other
        } else {
            self
        }
    }
}

/// The minimum distance as PostGIS's 3D functions measure it: in 3D if both geometries have Z,
/// otherwise in 2D. `None` if either is empty.
pub(crate) fn distance_3d(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<f64> {
    if has_z(geom1) && has_z(geom2) {
        nearest_parts(&parts(geom1), &parts(geom2)).map(|measure| measure.distance)
    } else {
        distance::distance(geom1, geom2)
    }
}

/// The maximum distance as PostGIS's 3D functions measure it: in 3D if both geometries have Z,
/// otherwise in 2D. `None` if either is empty.
pub(crate) fn max_distance_3d(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<f64> {
    if has_z(geom1) && has_z(geom2) {
        farthest_parts(&parts(geom1), &parts(geom2)).map(|measure| measure.distance)
    } else {
        distance::farthest(geom1, geom2).map(|measure| measure.distance)
    }
}

/// The nearest points as PostGIS's 3D functions find them, and whether they have Z. `None` if
/// either geometry is empty.
///
/// If only one geometry has Z, PostGIS takes the other's unknown Z to be any value: see
/// [`with_unknown_z`].
pub(crate) fn nearest_points_3d(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<(Measure3D, bool)> {
    match (has_z(geom1), has_z(geom2)) {
        (true, true) => nearest_parts(&parts(geom1), &parts(geom2)).map(|measure| (measure, true)),
        (false, false) => distance::nearest(geom1, geom2).map(|measure| (flat(measure), false)),
        (z1, _) => {
            let measure = distance::nearest(geom1, geom2)?;
            with_unknown_z(geom1, geom2, measure, z1, nearest_parts).map(|measure| (measure, true))
        }
    }
}

/// The farthest points as PostGIS's 3D functions find them, and whether they have Z. `None` if
/// either geometry is empty.
///
/// If only one geometry has Z, PostGIS takes the other's unknown Z to be any value: see
/// [`with_unknown_z`].
pub(crate) fn farthest_points_3d(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<(Measure3D, bool)> {
    match (has_z(geom1), has_z(geom2)) {
        (true, true) => farthest_parts(&parts(geom1), &parts(geom2)).map(|measure| (measure, true)),
        (false, false) => distance::farthest(geom1, geom2).map(|measure| (flat(measure), false)),
        (z1, _) => {
            let measure = distance::farthest(geom1, geom2)?;
            with_unknown_z(geom1, geom2, measure, z1, farthest_parts).map(|measure| (measure, true))
        }
    }
}

/// The points measured between a geometry with Z and one without, as PostGIS finds them: the
/// point of the 2D measure on the geometry without Z becomes a vertical segment over the Z range
/// of the other, and the points are measured between that and the geometry with Z, in 3D.
fn with_unknown_z(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
    measure: distance::Measure,
    z1: bool,
    measure_parts: fn(&[Part], &[Part]) -> Option<Measure3D>,
) -> Option<Measure3D> {
    let (with_z, (x, y)) = if z1 {
        (parts(geom1), measure.points[1])
    } else {
        (parts(geom2), measure.points[0])
    };
    let (min, max) = z_range(&with_z)?;
    let vertical = [Part::Line(vec![[x, y, min], [x, y, max]])];
    let measured = if z1 {
        measure_parts(&with_z, &vertical)
    } else {
        measure_parts(&vertical, &with_z)
    };
    measured.map(|measured| Measure3D {
        distance: measure.distance,
        points: measured.points,
    })
}

/// A 2D measure with its points at Z 0.
fn flat(measure: distance::Measure) -> Measure3D {
    let [p, q] = measure.points;
    Measure3D {
        distance: measure.distance,
        points: [[p.0, p.1, 0.0], [q.0, q.1, 0.0]],
    }
}

/// The n-dimensional distance of PostGIS's `<<->>`, combined with the difference in M if both
/// geometries have M. 0 if either is empty, as in PostGIS.
///
/// It is the distance between the nearest points, in 3D if both geometries have Z, otherwise in
/// 2D, so lines that cross are at distance 0. The M of a point or line is its M at the nearest
/// point, interpolated along the line. For other geometries PostGIS takes the gap between the
/// M ranges of their single-precision boxes.
pub(crate) fn distance_nd(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> f64 {
    let use_z = has_z(geom1) && has_z(geom2);
    let points = if use_z {
        nearest_parts(&parts(geom1), &parts(geom2)).map(|measure| measure.points)
    } else {
        distance::nearest(geom1, geom2).map(|measure| flat(measure).points)
    };
    let Some(points) = points else {
        return 0.0;
    };
    let distance = norm(sub(points[1], points[0]));
    if !(has_m(geom1) && has_m(geom2)) {
        return distance;
    }
    let m_difference = match (linear_m(geom1), linear_m(geom2)) {
        (Some(line1), Some(line2)) => (m_at(&line1, points[0]) - m_at(&line2, points[1])).abs(),
        _ => match (m_range(geom1), m_range(geom2)) {
            (Some((min1, max1)), Some((min2, max2))) => (min2 - max1).max(min1 - max2).max(0.0),
            _ => 0.0,
        },
    };
    (distance * distance + m_difference * m_difference).sqrt()
}

/// Whether a geometry has M.
fn has_m(geom: &impl GeometryTrait<T = f64>) -> bool {
    matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm)
}

/// The vertices of a point or a line, with Z (0 if none) and M; `None` for other geometries.
fn linear_m(geom: &impl GeometryTrait<T = f64>) -> Option<Vec<[f64; 4]>> {
    let xyzm = |coord: &dyn CoordTrait<T = f64>| {
        let (z, m) = match coord.dim() {
            Dimensions::Xyzm => (coord.nth_or_panic(2), coord.nth_or_panic(3)),
            _ => (0.0, coord.nth_or_panic(2)),
        };
        [coord.x(), coord.y(), z, m]
    };
    match geom.as_type() {
        GeometryType::Point(point) => Some(point.coord().iter().map(|c| xyzm(c)).collect()),
        GeometryType::LineString(line) => Some(line.coords().map(|c| xyzm(&c)).collect()),
        _ => None,
    }
}

/// The M of a line at `p`, a point of it: interpolated along the segment nearest to `p` in 2D,
/// as PostGIS locates it even in 3D.
fn m_at(line: &[[f64; 4]], p: Xyz) -> f64 {
    let point = |v: &[f64; 4]| [v[0], v[1], 0.0];
    let p = [p[0], p[1], 0.0];
    let mut best: Option<(f64, f64)> = None;
    if let [only] = line {
        return only[3];
    }
    for segment in line.windows(2) {
        let (a, b) = (point(&segment[0]), point(&segment[1]));
        let ab = sub(b, a);
        let length_squared = dot(ab, ab);
        let r = if length_squared == 0.0 {
            0.0
        } else {
            dot(sub(p, a), ab) / length_squared
        };
        // At an end, its own M.
        let (q, m) = if r <= 0.0 {
            (a, segment[0][3])
        } else if r >= 1.0 {
            (b, segment[1][3])
        } else {
            (
                add(a, scale(ab, r)),
                segment[0][3] + r * (segment[1][3] - segment[0][3]),
            )
        };
        let distance = norm(sub(q, p));
        if best.is_none_or(|(best, _)| distance < best) {
            best = Some((distance, m));
        }
    }
    best.map_or(0.0, |(_, m)| m)
}

/// The M range of a geometry as PostGIS boxes it, rounded outward to single precision; `None`
/// if it is empty.
fn m_range(geom: &impl GeometryTrait<T = f64>) -> Option<(f64, f64)> {
    let mut bounds = BoundingRect::new(true);
    bounds.add_geometry(geom);
    bounds
        .m_range()
        .map(|(min, max)| (round_down(min), round_up(max)))
}

/// Whether a geometry has Z. Its points have Z then, or are taken as Z 0.
fn has_z(geom: &impl GeometryTrait<T = f64>) -> bool {
    matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm)
}

/// The Z range of the parts of a geometry, or `None` if there are none.
fn z_range(parts: &[Part]) -> Option<(f64, f64)> {
    parts
        .iter()
        .flat_map(|part| part.vertices())
        .fold(None, |range, p| {
            Some(range.map_or((p[2], p[2]), |(min, max): (f64, f64)| {
                (min.min(p[2]), max.max(p[2]))
            }))
        })
}

/// The minimum 3D distance between the parts of two geometries and the nearest points, or
/// `None` if either has none.
fn nearest_parts(parts1: &[Part], parts2: &[Part]) -> Option<Measure3D> {
    let mut best: Option<Measure3D> = None;
    for part1 in parts1 {
        for part2 in parts2 {
            let measure = part_nearest(part1, part2);
            let best = best.insert(best.map_or(measure, |best| best.nearer(measure)));
            if best.distance == 0.0 {
                return Some(*best);
            }
        }
    }
    best
}

/// The maximum 3D distance between the parts of two geometries and the farthest points, or
/// `None` if either has none. The farthest points are vertices, of the polygons' shells.
fn farthest_parts(parts1: &[Part], parts2: &[Part]) -> Option<Measure3D> {
    let mut best: Option<Measure3D> = None;
    for part1 in parts1 {
        for part2 in parts2 {
            for &p in part1.outline() {
                for &q in part2.outline() {
                    let measure = Measure3D::between(p, q);
                    if best.is_none_or(|best| measure.distance > best.distance) {
                        best = Some(measure);
                    }
                }
            }
        }
    }
    best
}

/// A non-empty basic part of a geometry.
enum Part {
    Point(Xyz),
    Line(Vec<Xyz>),
    Polygon(Polygon3D),
}

impl Part {
    /// The point, the line's points, or the polygon's shell.
    fn outline(&self) -> &[Xyz] {
        match self {
            Part::Point(p) => std::slice::from_ref(p),
            Part::Line(line) => line,
            Part::Polygon(polygon) => &polygon.rings[0],
        }
    }

    fn vertices(&self) -> Vec<Xyz> {
        match self {
            Part::Polygon(polygon) => polygon.rings.iter().flatten().copied().collect(),
            part => part.outline().to_vec(),
        }
    }
}

/// A polygon and its plane. The first ring is the shell.
struct Polygon3D {
    rings: Vec<Vec<Xyz>>,
    /// A point of the plane, and its normal; `None` if the shell spans no plane.
    plane: Option<(Xyz, Xyz)>,
    /// The axis dropped to test for points inside: the normal's largest.
    axis: usize,
}

impl Polygon3D {
    fn new(rings: Vec<Vec<Xyz>>) -> Self {
        let shell = &rings[0];
        let vertices = if shell.len() > 1 && shell.first() == shell.last() {
            &shell[..shell.len() - 1]
        } else {
            &shell[..]
        };
        let count = vertices.len() as f64;
        let center = vertices
            .iter()
            .fold([0.0; 3], |sum, p| add(sum, *p))
            .map(|sum| sum / count);
        // Newell's method.
        let mut normal = [0.0; 3];
        for segment in shell.windows(2) {
            let (a, b) = (segment[0], segment[1]);
            normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
            normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
            normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let axis = (0..3)
            .max_by(|&i, &j| normal[i].abs().total_cmp(&normal[j].abs()))
            .expect("three axes");
        let plane = (normal != [0.0; 3]).then_some((center, normal));
        Polygon3D { rings, plane, axis }
    }

    /// The signed distance of `p` from the plane, in units of the normal's length.
    fn side(&self, p: Xyz) -> Option<f64> {
        self.plane
            .map(|(center, normal)| dot(normal, sub(p, center)))
    }

    /// Where the segment `a`-`b` passes through the polygon, if it does: through the plane from
    /// one side to the other, inside the polygon.
    fn crossing(&self, a: Xyz, b: Xyz) -> Option<Xyz> {
        let (side_a, side_b) = (self.side(a)?, self.side(b)?);
        if side_a * side_b >= 0.0 {
            return None;
        }
        // Divided last, which puts the crossing where PostGIS does.
        let crossing = add(a, sub(b, a).map(|d| d * side_a / (side_a - side_b)));
        self.contains(crossing).then_some(crossing)
    }

    /// The projection of `p` onto the plane.
    fn project(&self, p: Xyz) -> Option<Xyz> {
        let (center, normal) = self.plane?;
        let t = dot(normal, sub(center, p)) / dot(normal, normal);
        Some(add(p, scale(normal, t)))
    }

    /// Whether `p`, a point of the plane, is inside the polygon or on its boundary, by the
    /// winding number rule in the plane's projection along `axis`.
    fn contains(&self, p: Xyz) -> bool {
        let (i, j) = match self.axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        let flat = |q: &Xyz| (q[i], q[j]);
        let (shell, holes) = self.rings.split_first().expect("a polygon has a shell");
        let p = flat(&p);
        in_ring(p, shell.iter().map(flat))
            && !holes
                .iter()
                .any(|hole| strictly_in_ring(p, hole.iter().map(flat)))
    }
}

/// The non-empty points, lines and polygons of a geometry, collections flattened. Without Z,
/// points are at Z 0.
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
        // A box3d is its surface: the bottom, the sides and the top, unless it is flat.
        GeometryType::Rect(rect) => {
            let (min, max) = (xyz(&rect.min()), xyz(&rect.max()));
            let corner = |x: usize, y: usize, z: usize| {
                [
                    [min[0], max[0]][x],
                    [min[1], max[1]][y],
                    [min[2], max[2]][z],
                ]
            };
            let face = |corners: [(usize, usize, usize); 4]| {
                let mut ring: Vec<Xyz> = corners.iter().map(|&(x, y, z)| corner(x, y, z)).collect();
                ring.push(ring[0]);
                Part::Polygon(Polygon3D::new(vec![ring]))
            };
            parts.push(face([(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]));
            if min[2] != max[2] {
                parts.push(face([(0, 0, 0), (0, 0, 1), (1, 0, 1), (1, 0, 0)]));
                parts.push(face([(1, 0, 0), (1, 0, 1), (1, 1, 1), (1, 1, 0)]));
                parts.push(face([(1, 1, 0), (1, 1, 1), (0, 1, 1), (0, 1, 0)]));
                parts.push(face([(0, 1, 0), (0, 1, 1), (0, 0, 1), (0, 0, 0)]));
                parts.push(face([(0, 0, 1), (0, 1, 1), (1, 1, 1), (1, 0, 1)]));
            }
        }
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| xyz(&coord));
            parts.push(Part::Polygon(Polygon3D::new(vec![vec![a, b, c, a]])));
        }
        GeometryType::Line(line) => {
            parts.push(Part::Line(vec![xyz(&line.start()), xyz(&line.end())]));
        }
    }
}

fn xyz(coord: &impl CoordTrait<T = f64>) -> Xyz {
    let z = match coord.dim() {
        Dimensions::Xyz | Dimensions::Xyzm => coord.nth_or_panic(2),
        _ => 0.0,
    };
    [coord.x(), coord.y(), z]
}

fn point_part(point: &impl PointTrait<T = f64>) -> Option<Part> {
    point.coord().map(|coord| Part::Point(xyz(&coord)))
}

fn line_part(line: &impl LineStringTrait<T = f64>) -> Option<Part> {
    let coords: Vec<Xyz> = line.coords().map(|coord| xyz(&coord)).collect();
    (!coords.is_empty()).then_some(Part::Line(coords))
}

fn polygon_part(polygon: &impl PolygonTrait<T = f64>) -> Option<Part> {
    let rings: Vec<Vec<Xyz>> = polygon
        .exterior()
        .into_iter()
        .chain(polygon.interiors())
        .map(|ring| ring.coords().map(|coord| xyz(&coord)).collect::<Vec<_>>())
        .collect();
    (!rings.first()?.is_empty()).then(|| Part::Polygon(Polygon3D::new(rings)))
}

fn part_nearest(part1: &Part, part2: &Part) -> Measure3D {
    match (part1, part2) {
        (Part::Point(p), Part::Point(q)) => Measure3D::between(*p, *q),
        (Part::Point(p), Part::Line(line)) => point_line(*p, line),
        (Part::Line(line), Part::Point(p)) => point_line(*p, line).swap(),
        (Part::Line(line1), Part::Line(line2)) => line_line(line1, line2),
        (Part::Point(p), Part::Polygon(polygon)) => point_polygon(*p, polygon),
        (Part::Polygon(polygon), Part::Point(p)) => point_polygon(*p, polygon).swap(),
        (Part::Line(line), Part::Polygon(polygon)) => line_polygon(line, polygon),
        (Part::Polygon(polygon), Part::Line(line)) => line_polygon(line, polygon).swap(),
        // The nearest points of two polygons are on the boundary of one of them, as for a line
        // and a polygon. Where the second's rings pass through the first, PostGIS reports that
        // before anything else.
        (Part::Polygon(polygon1), Part::Polygon(polygon2)) => {
            let crossing = polygon2.rings.iter().find_map(|ring| {
                ring.windows(2)
                    .find_map(|segment| polygon1.crossing(segment[0], segment[1]))
            });
            if let Some(crossing) = crossing {
                return Measure3D::between(crossing, crossing);
            }
            let mut best: Option<Measure3D> = None;
            let measures = polygon1
                .rings
                .iter()
                .map(|ring| line_polygon(ring, polygon2))
                .chain(
                    polygon2
                        .rings
                        .iter()
                        .map(|ring| line_polygon(ring, polygon1).swap()),
                );
            for measure in measures {
                let best = best.insert(best.map_or(measure, |best| best.nearer(measure)));
                if best.distance == 0.0 {
                    break;
                }
            }
            best.expect("a polygon has a shell")
        }
    }
}

/// The measure from `p` to the nearest point of the line.
fn point_line(p: Xyz, line: &[Xyz]) -> Measure3D {
    match line {
        [only] => Measure3D::between(p, *only),
        _ => line
            .windows(2)
            .map(|segment| point_segment(p, segment[0], segment[1]))
            .reduce(Measure3D::nearer)
            .expect("a line has a segment"),
    }
}

/// The measure from `p` to the segment `a`-`b`: to its projection onto the segment's line, or
/// to the nearer end if that falls outside.
fn point_segment(p: Xyz, a: Xyz, b: Xyz) -> Measure3D {
    let ab = sub(b, a);
    let length_squared = dot(ab, ab);
    if length_squared == 0.0 {
        return Measure3D::between(p, a);
    }
    let r = dot(sub(p, a), ab) / length_squared;
    if r <= 0.0 {
        return Measure3D::between(p, a);
    }
    if r >= 1.0 {
        return Measure3D::between(p, b);
    }
    Measure3D::between(p, add(a, scale(ab, r)))
}

fn line_line(line1: &[Xyz], line2: &[Xyz]) -> Measure3D {
    match (line1, line2) {
        ([p], line) => point_line(*p, line),
        (line, [p]) => point_line(*p, line).swap(),
        _ => {
            let mut best: Option<Measure3D> = None;
            for a in line1.windows(2) {
                for b in line2.windows(2) {
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

/// The measure between the segments `a`-`b` and `c`-`d`.
///
/// The closest points of the segments' lines, `a + s (b - a)` and `c + t (d - c)`, if both are
/// on the segments. Otherwise the nearest of the ends of one segment to the other, `c` and `d`
/// first, as PostGIS's results show. Segments closer to parallel than PostGIS's 1e-8 are
/// measured from `a`, or from the end of `c`-`d` that `a`'s projection falls beyond.
fn segment_segment(a: Xyz, b: Xyz, c: Xyz, d: Xyz) -> Measure3D {
    let (u, v, w) = (sub(b, a), sub(d, c), sub(a, c));
    let (uu, uv, vv, uw, vw) = (dot(u, u), dot(u, v), dot(v, v), dot(u, w), dot(v, w));
    if uu == 0.0 {
        return point_segment(a, c, d);
    }
    if vv == 0.0 {
        return point_segment(c, a, b).swap();
    }
    let denominator = uu * vv - uv * uv;
    if denominator < 1e-8 {
        // The fraction along `a`-`b` nearest to the point of `c`-`d` at `end`.
        let fraction = |numerator: f64| (numerator / uu).clamp(0.0, 1.0);
        let (s, t) = if vw < 0.0 {
            (fraction(-uw), 0.0)
        } else if vw > vv {
            (fraction(uv - uw), 1.0)
        } else {
            (0.0, vw / vv)
        };
        return Measure3D::between(add(a, scale(u, s)), add(c, scale(v, t)));
    }
    let s = (uv * vw - vv * uw) / denominator;
    let t = (uu * vw - uv * uw) / denominator;
    if !(0.0..=1.0).contains(&s) || !(0.0..=1.0).contains(&t) {
        return point_segment(c, a, b)
            .swap()
            .nearer(point_segment(d, a, b).swap())
            .nearer(point_segment(a, c, d))
            .nearer(point_segment(b, c, d));
    }
    Measure3D::between(add(a, scale(u, s)), add(c, scale(v, t)))
}

/// The measure from `p` to the polygon: to its projection onto the plane if that is inside the
/// polygon, otherwise to the nearest ring.
fn point_polygon(p: Xyz, polygon: &Polygon3D) -> Measure3D {
    if let Some(q) = polygon.project(p).filter(|&q| polygon.contains(q)) {
        return Measure3D::between(p, q);
    }
    polygon
        .rings
        .iter()
        .map(|ring| point_line(p, ring))
        .reduce(Measure3D::nearer)
        .expect("a polygon has a shell")
}

/// The measure from the line to the polygon, segment by segment: 0 where a segment passes
/// through the polygon, otherwise the nearest of the segment's ends to the plane, where they
/// project inside the polygon, and of the segment to the rings.
fn line_polygon(line: &[Xyz], polygon: &Polygon3D) -> Measure3D {
    if let [p] = line {
        return point_polygon(*p, polygon);
    }
    let mut best: Option<Measure3D> = None;
    let mut consider = |measure: Measure3D| {
        let best = best.insert(best.map_or(measure, |best| best.nearer(measure)));
        best.distance == 0.0
    };
    for segment in line.windows(2) {
        let (a, b) = (segment[0], segment[1]);
        for end in [a, b] {
            if let Some(q) = polygon.project(end).filter(|&q| polygon.contains(q))
                && consider(Measure3D::between(end, q))
            {
                return best.expect("just considered");
            }
        }
        if let Some(crossing) = polygon.crossing(a, b)
            && consider(Measure3D::between(crossing, crossing))
        {
            return best.expect("just considered");
        }
        for ring in &polygon.rings {
            if consider(line_line(&[a, b], ring)) {
                return best.expect("just considered");
            }
        }
    }
    best.expect("a line has a segment")
}

/// The winding number rule: whether the ring winds around `p` or passes through it.
fn in_ring(p: (f64, f64), ring: impl Iterator<Item = (f64, f64)>) -> bool {
    winding(p, ring).is_none_or(|winding| winding != 0)
}

/// Whether the ring winds around `p`, not counting its boundary.
fn strictly_in_ring(p: (f64, f64), ring: impl Iterator<Item = (f64, f64)>) -> bool {
    winding(p, ring).is_some_and(|winding| winding != 0)
}

/// The winding number of the ring around `p`, or `None` if `p` is on the ring.
fn winding(p: (f64, f64), ring: impl Iterator<Item = (f64, f64)>) -> Option<i32> {
    let ring: Vec<(f64, f64)> = ring.collect();
    let mut winding = 0;
    for segment in ring.windows(2) {
        let (a, b) = (segment[0], segment[1]);
        let side = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        let within = |s: f64, e: f64, v: f64| s.min(e) <= v && v <= s.max(e);
        if side == 0.0 && within(a.0, b.0, p.0) && within(a.1, b.1, p.1) {
            return None;
        }
        if a.1 <= p.1 {
            if b.1 > p.1 && side > 0.0 {
                winding += 1;
            }
        } else if b.1 <= p.1 && side < 0.0 {
            winding -= 1;
        }
    }
    Some(winding)
}

fn add(a: Xyz, b: Xyz) -> Xyz {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Xyz, b: Xyz) -> Xyz {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: Xyz, k: f64) -> Xyz {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: Xyz, b: Xyz) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: Xyz) -> f64 {
    dot(a, a).sqrt()
}
