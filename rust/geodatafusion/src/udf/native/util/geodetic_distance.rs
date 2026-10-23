//! The nearest points of two geography geometries on the sphere: what PostGIS measures the
//! distance between, on the sphere or the spheroid.
//!
//! Points become unit vectors. A great-circle arc's nearest point to a point is the point's
//! projection onto the arc's plane, if that falls on the arc, or else the nearer end. Arcs that
//! cross are at distance zero, and so is anything inside a polygon. Inside means a winding number
//! other than zero around the point, which is the side of the ring away from the point's
//! antipode; for rings smaller than a hemisphere that is the side they enclose.

use std::f64::consts::PI;

use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};
use wkt::types::Coord;

pub(crate) type Vector = [f64; 3];

/// A point on the sphere: longitude and latitude in radians, and its unit vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spot {
    pub(crate) lon: f64,
    pub(crate) lat: f64,
    vector: Vector,
}

impl Spot {
    /// The point at a longitude and latitude in degrees, turned into radians as PostGIS does.
    pub(crate) fn from_degrees(lon: f64, lat: f64) -> Self {
        Self::new(lon * PI / 180.0, lat * PI / 180.0)
    }

    fn new(lon: f64, lat: f64) -> Self {
        let (sin_lat, cos_lat) = lat.sin_cos();
        let (sin_lon, cos_lon) = lon.sin_cos();
        Self {
            lon,
            lat,
            vector: [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat],
        }
    }

    #[cfg_attr(
        not(feature = "geos-3_11"),
        expect(
            dead_code,
            reason = "only ST_Centroid, backed by GEOS, takes geography centroids"
        )
    )]
    pub(crate) fn vector(&self) -> Vector {
        self.vector
    }

    /// The point a unit vector points at.
    pub(crate) fn from_vector(vector: Vector) -> Self {
        let [x, y, z] = vector;
        Self {
            lon: y.atan2(x),
            lat: z.asin(),
            vector,
        }
    }
}

/// The nearest points of two geometries, the first's and the second's, and the angle between
/// them at the centre of the sphere.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Nearest {
    pub(crate) angle: f64,
    pub(crate) points: [Spot; 2],
}

/// The parts of a geometry the distance is between.
#[derive(Default)]
struct Shapes {
    points: Vec<Spot>,
    /// Lines and polygon rings, as their vertices.
    lines: Vec<Vec<Spot>>,
    /// Polygons, as their rings, the shell first.
    polygons: Vec<Vec<Vec<Spot>>>,
}

impl Shapes {
    fn new(geom: &impl GeometryTrait<T = f64>) -> Self {
        let mut shapes = Self::default();
        shapes.add(geom);
        shapes
    }

    fn is_empty(&self) -> bool {
        self.points.is_empty() && self.lines.iter().all(Vec::is_empty)
    }

    fn add(&mut self, geom: &impl GeometryTrait<T = f64>) {
        match geom.as_type() {
            GeometryType::Point(point) => self.points.extend(point.coord().map(|c| spot(&c))),
            GeometryType::MultiPoint(points) => {
                for point in points.points() {
                    self.points.extend(point.coord().map(|c| spot(&c)));
                }
            }
            GeometryType::LineString(line) => self.add_line(line),
            GeometryType::MultiLineString(lines) => {
                for line in lines.line_strings() {
                    self.add_line(&line);
                }
            }
            GeometryType::Polygon(polygon) => self.add_polygon(polygon),
            GeometryType::MultiPolygon(polygons) => {
                for polygon in polygons.polygons() {
                    self.add_polygon(&polygon);
                }
            }
            GeometryType::GeometryCollection(collection) => {
                for member in collection.geometries() {
                    self.add(&member);
                }
            }
            GeometryType::Line(line) => self
                .lines
                .push(vec![spot(&line.start()), spot(&line.end())]),
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
                self.lines.push(ring.clone());
                self.polygons.push(vec![ring]);
            }
            GeometryType::Triangle(triangle) => {
                let [a, b, c] = triangle.coords().map(|coord| spot(&coord));
                let ring = vec![a, b, c, a];
                self.lines.push(ring.clone());
                self.polygons.push(vec![ring]);
            }
        }
    }

    fn add_line(&mut self, line: &impl LineStringTrait<T = f64>) {
        let spots: Vec<Spot> = line.coords().map(|coord| spot(&coord)).collect();
        // A line of one point is that point.
        match spots.as_slice() {
            [] => {}
            [only] => self.points.push(*only),
            _ => self.lines.push(spots),
        }
    }

    fn add_polygon(&mut self, polygon: &impl PolygonTrait<T = f64>) {
        let rings: Vec<Vec<Spot>> = polygon
            .exterior()
            .into_iter()
            .chain(polygon.interiors())
            .map(|ring| ring.coords().map(|coord| spot(&coord)).collect::<Vec<_>>())
            .filter(|ring| !ring.is_empty())
            .collect();
        if rings.is_empty() {
            return;
        }
        self.lines.extend(rings.iter().cloned());
        self.polygons.push(rings);
    }

    /// The first point of each part, to test for being inside a polygon of the other geometry.
    fn first_points(&self) -> impl Iterator<Item = Spot> + '_ {
        self.points
            .iter()
            .copied()
            .chain(self.lines.iter().filter_map(|line| line.first().copied()))
    }

    /// Whether `spot` is inside one of the polygons.
    fn contains(&self, spot: &Spot) -> bool {
        self.polygons.iter().any(|rings| {
            let mut rings = rings.iter();
            rings.next().is_some_and(|shell| in_ring(shell, spot))
                && !rings.any(|hole| in_ring(hole, spot))
        })
    }

    /// The edges of the lines and rings.
    fn edges(&self) -> impl Iterator<Item = [Spot; 2]> + '_ {
        self.lines
            .iter()
            .flat_map(|line| line.windows(2).map(|pair| [pair[0], pair[1]]))
    }
}

fn spot(coord: &impl CoordTrait<T = f64>) -> Spot {
    Spot::from_degrees(coord.x(), coord.y())
}

/// The nearest points of two geometries on the sphere; `None` if either is empty.
pub(crate) fn nearest(
    a: &impl GeometryTrait<T = f64>,
    b: &impl GeometryTrait<T = f64>,
) -> Option<Nearest> {
    let (a, b) = (Shapes::new(a), Shapes::new(b));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    // A part inside a polygon of the other geometry is at distance zero.
    for spot in b.first_points() {
        if a.contains(&spot) {
            return Some(zero(spot));
        }
    }
    for spot in a.first_points() {
        if b.contains(&spot) {
            return Some(zero(spot));
        }
    }
    let mut best: Option<Nearest> = None;
    let mut consider = |candidate: Nearest| {
        if best.is_none_or(|best| candidate.angle < best.angle) {
            best = Some(candidate);
        }
    };
    for p in &a.points {
        for q in &b.points {
            consider(Nearest {
                angle: angle(&p.vector, &q.vector),
                points: [*p, *q],
            });
        }
        for edge in b.edges() {
            let q = nearest_on_arc(p, &edge);
            consider(Nearest {
                angle: angle(&p.vector, &q.vector),
                points: [*p, q],
            });
        }
    }
    for edge in a.edges() {
        for q in &b.points {
            let p = nearest_on_arc(q, &edge);
            consider(Nearest {
                angle: angle(&p.vector, &q.vector),
                points: [p, *q],
            });
        }
        for other in b.edges() {
            if let Some(crossing) = crossing(&edge, &other) {
                return Some(zero(crossing));
            }
            for (p, q) in [
                (edge[0], nearest_on_arc(&edge[0], &other)),
                (edge[1], nearest_on_arc(&edge[1], &other)),
            ] {
                consider(Nearest {
                    angle: angle(&p.vector, &q.vector),
                    points: [p, q],
                });
            }
            for (q, p) in [
                (other[0], nearest_on_arc(&other[0], &edge)),
                (other[1], nearest_on_arc(&other[1], &edge)),
            ] {
                consider(Nearest {
                    angle: angle(&p.vector, &q.vector),
                    points: [p, q],
                });
            }
        }
    }
    best
}

/// The coordinates, as longitude and latitude in degrees, with each edge bisected at its
/// great-circle midpoint, and each half again, until no part is longer than `max_length` on a
/// sphere of `radius`, as PostGIS segmentizes geography. Z and M are interpolated linearly, and
/// a repeated point is dropped, unless the line would be left with one. An edge between
/// antipodes, which has no one great circle, is an error.
pub(crate) fn segmentize(
    coords: Vec<Coord<f64>>,
    max_length: f64,
    radius: f64,
) -> Result<Vec<Coord<f64>>, &'static str> {
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    let last_index = coords.len().saturating_sub(1);
    for (index, end) in coords.into_iter().enumerate() {
        let Some(start) = out.last().copied() else {
            out.push(end);
            continue;
        };
        if end == start {
            if index == last_index && out.len() < 2 {
                out.push(end);
            }
            continue;
        }
        let (a, b) = (
            Spot::from_degrees(start.x, start.y),
            Spot::from_degrees(end.x, end.y),
        );
        bisect(&mut out, (a, start), (b, end), max_length, radius)?;
        out.push(end);
    }
    Ok(out)
}

/// Pushes the points that split the edge from `a` to `b` into parts no longer than `max_length`,
/// not `a` and `b` themselves.
fn bisect(
    out: &mut Vec<Coord<f64>>,
    (a, a_coord): (Spot, Coord<f64>),
    (b, b_coord): (Spot, Coord<f64>),
    max_length: f64,
    radius: f64,
) -> Result<(), &'static str> {
    if radius * vincenty_angle(&a, &b) <= max_length {
        return Ok(());
    }
    let sum = [
        a.vector[0] + b.vector[0],
        a.vector[1] + b.vector[1],
        a.vector[2] + b.vector[2],
    ];
    let length = norm(&sum);
    // Antipodes in floating point leave a sum of about 1e-16.
    if length < 1e-12 {
        return Err("Antipodal (180 degrees long) edge detected!");
    }
    // Divided component by component, which rounds as PostGIS's results show.
    let middle = Spot::from_vector(normalize(&sum, length));
    let half = |p: Option<f64>, q: Option<f64>| p.zip(q).map(|(p, q)| (p + q) / 2.0);
    let middle_coord = Coord {
        x: middle.lon * 180.0 / PI,
        y: middle.lat * 180.0 / PI,
        z: half(a_coord.z, b_coord.z),
        m: half(a_coord.m, b_coord.m),
    };
    bisect(
        out,
        (a, a_coord),
        (middle, middle_coord),
        max_length,
        radius,
    )?;
    out.push(middle_coord);
    bisect(
        out,
        (middle, middle_coord),
        (b, b_coord),
        max_length,
        radius,
    )
}

/// The angle between two points by Vincenty's formula on their longitudes and latitudes, as
/// PostGIS measures on the sphere.
fn vincenty_angle(p: &Spot, q: &Spot) -> f64 {
    let delta = q.lon - p.lon;
    let (sin1, cos1) = p.lat.sin_cos();
    let (sin2, cos2) = q.lat.sin_cos();
    let (sin_delta, cos_delta) = delta.sin_cos();
    let a = cos2 * sin_delta;
    let b = cos1 * sin2 - sin1 * cos2 * cos_delta;
    (a * a + b * b)
        .sqrt()
        .atan2(sin1 * sin2 + cos1 * cos2 * cos_delta)
}

fn zero(spot: Spot) -> Nearest {
    Nearest {
        angle: 0.0,
        points: [spot, spot],
    }
}

/// The point of the arc from `edge[0]` to `edge[1]` nearest to `p`.
fn nearest_on_arc(p: &Spot, edge: &[Spot; 2]) -> Spot {
    let [a, b] = edge;
    let normal = arc_normal(a, b);
    let length = norm(&normal);
    if length == 0.0 {
        // A degenerate edge, or one between antipodes, which has no one great circle.
        return nearer(p, a, b);
    }
    let normal = normalize(&normal, length);
    let offset = dot(&p.vector, &normal);
    let projected = [
        p.vector[0] - offset * normal[0],
        p.vector[1] - offset * normal[1],
        p.vector[2] - offset * normal[2],
    ];
    let projected_length = norm(&projected);
    if projected_length == 0.0 {
        // The point is a pole of the arc's great circle: every point of it is as far.
        return *a;
    }
    let c = normalize(&projected, projected_length);
    if on_arc(&c, a, b, &normal) {
        Spot::from_vector(c)
    } else {
        nearer(p, a, b)
    }
}

fn nearer(p: &Spot, a: &Spot, b: &Spot) -> Spot {
    if angle(&p.vector, &a.vector) <= angle(&p.vector, &b.vector) {
        *a
    } else {
        *b
    }
}

/// Whether `c`, on the great circle of `a` and `b` with `normal`, lies between them.
fn on_arc(c: &Vector, a: &Spot, b: &Spot, normal: &Vector) -> bool {
    dot(&cross(&a.vector, c), normal) >= 0.0 && dot(&cross(c, &b.vector), normal) >= 0.0
}

/// Where two arcs cross, if they do.
fn crossing(e: &[Spot; 2], f: &[Spot; 2]) -> Option<Spot> {
    let n1 = arc_normal(&e[0], &e[1]);
    let n2 = arc_normal(&f[0], &f[1]);
    if norm(&n1) == 0.0 || norm(&n2) == 0.0 {
        return None;
    }
    let line = cross(&n1, &n2);
    let length = norm(&line);
    if length == 0.0 {
        // The same great circle: overlapping arcs share an end inside the other, which the
        // nearest-point search finds at distance zero.
        return None;
    }
    let candidate = normalize(&line, length);
    [candidate, scale(&candidate, -1.0)]
        .into_iter()
        .find(|c| on_arc(c, &e[0], &e[1], &n1) && on_arc(c, &f[0], &f[1], &n2))
        .map(Spot::from_vector)
}

/// The normal of the great circle from `a` to `b`: `a × b`, computed from the longitudes and
/// latitudes without the cancellation the cross product of nearly equal vectors suffers, as in
/// the Aviation Formulary of Ed Williams (whose longitudes are positive west, hence the signs).
fn arc_normal(a: &Spot, b: &Spot) -> Vector {
    let (sum, half) = ((a.lon + b.lon) / 2.0, (a.lon - b.lon) / 2.0);
    let (sin_sum, cos_sum) = sum.sin_cos();
    let (sin_half, cos_half) = half.sin_cos();
    let (lat_difference, lat_sum) = ((a.lat - b.lat).sin(), (a.lat + b.lat).sin());
    [
        -(lat_difference * sin_sum * cos_half - lat_sum * cos_sum * sin_half),
        lat_difference * cos_sum * cos_half + lat_sum * sin_sum * sin_half,
        -(a.lat.cos() * b.lat.cos() * (a.lon - b.lon).sin()),
    ]
}

/// Whether `p` is inside a ring: the ring winds around it, as seen in the plane tangent at it.
fn in_ring(ring: &[Spot], p: &Spot) -> bool {
    let mut total = 0.0;
    for pair in ring.windows(2) {
        let (ta, tb) = (
            tangent(&p.vector, &pair[0].vector),
            tangent(&p.vector, &pair[1].vector),
        );
        total += dot(&p.vector, &cross(&ta, &tb)).atan2(dot(&ta, &tb));
    }
    total.abs() > PI
}

/// The direction of `v` from `p` in the plane tangent to the sphere at `p`.
fn tangent(p: &Vector, v: &Vector) -> Vector {
    let along = dot(p, v);
    [
        v[0] - along * p[0],
        v[1] - along * p[1],
        v[2] - along * p[2],
    ]
}

fn angle(u: &Vector, v: &Vector) -> f64 {
    norm(&cross(u, v)).atan2(dot(u, v))
}

fn cross(u: &Vector, v: &Vector) -> Vector {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn dot(u: &Vector, v: &Vector) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

pub(crate) fn norm(v: &Vector) -> f64 {
    dot(v, v).sqrt()
}

/// The vector divided by its `length`, component by component, as PostGIS normalises.
pub(crate) fn normalize(v: &Vector, length: f64) -> Vector {
    [v[0] / length, v[1] / length, v[2] / length]
}

fn scale(v: &Vector, factor: f64) -> Vector {
    [v[0] * factor, v[1] * factor, v[2] * factor]
}
