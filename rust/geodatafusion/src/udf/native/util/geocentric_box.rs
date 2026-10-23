//! PostGIS's bounding boxes of geography: boxes in geocentric coordinates, on the unit sphere.
//!
//! A point is its unit vector. A great-circle edge reaches beyond its ends where it passes
//! the point of its circle furthest along an axis, so the box takes those points too. A
//! polygon's box spans the sphere's axis on a side when the box of its rings straddles the
//! two other axes: the rings go around that axis, so the polygon may cover where it leaves the
//! sphere. PostGIS decides by the box alone, so a ring around a pole that doesn't enclose it, or
//! a hole around it, extends the box all the same. As PostGIS stores them, the boxes are
//! rounded outward to single precision.

use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};

use crate::udf::native::util::float_box::{round_down, round_up};
use crate::udf::native::util::geodetic_distance::{
    Spot, Vector, arc_normal, norm, normalize, on_arc, spot,
};

/// A box in geocentric coordinates: the range of x, y and z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GeocentricBox([(f64, f64); 3]);

impl GeocentricBox {
    /// The box of a geography, rounded outward to single precision; `None` if it is EMPTY.
    pub(crate) fn new(geom: &impl GeometryTrait<T = f64>) -> Option<Self> {
        let mut bounds = Bounds::default();
        bounds.add_geometry(geom);
        let ranges = bounds.0?;
        Some(Self(
            ranges.map(|(min, max)| (round_down(min), round_up(max))),
        ))
    }

    /// The box of both.
    pub(crate) fn union(&self, other: &Self) -> Self {
        Self(std::array::from_fn(|axis| {
            (
                self.0[axis].0.min(other.0[axis].0),
                self.0[axis].1.max(other.0[axis].1),
            )
        }))
    }

    /// The ranges of x, y and z.
    pub(crate) fn ranges(&self) -> [(f64, f64); 3] {
        self.0
    }

    /// Whether the boxes share a point.
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(&other.0)
            .all(|(a, b)| a.0 <= b.1 && b.0 <= a.1)
    }
}

/// The ranges seen so far, exact.
#[derive(Debug, Default, Clone, Copy)]
struct Bounds(Option<[(f64, f64); 3]>);

impl Bounds {
    fn add_vector(&mut self, v: &Vector) {
        let ranges = self
            .0
            .get_or_insert([(v[0], v[0]), (v[1], v[1]), (v[2], v[2])]);
        for (range, value) in ranges.iter_mut().zip(v) {
            range.0 = range.0.min(*value);
            range.1 = range.1.max(*value);
        }
    }

    fn add_bounds(&mut self, other: &Bounds) {
        // Two opposite corners cover the ranges.
        if let Some(ranges) = other.0 {
            self.add_vector(&ranges.map(|range| range.0));
            self.add_vector(&ranges.map(|range| range.1));
        }
    }

    fn add_geometry(&mut self, geom: &impl GeometryTrait<T = f64>) {
        match geom.as_type() {
            GeometryType::Point(point) => {
                if let Some(coord) = point.coord() {
                    self.add_vector(&spot(&coord).vector());
                }
            }
            GeometryType::MultiPoint(points) => {
                for point in points.points() {
                    if let Some(coord) = point.coord() {
                        self.add_vector(&spot(&coord).vector());
                    }
                }
            }
            GeometryType::LineString(line) => self.add_line(line.coords().map(|c| spot(&c))),
            GeometryType::MultiLineString(lines) => {
                for line in lines.line_strings() {
                    self.add_line(line.coords().map(|c| spot(&c)));
                }
            }
            GeometryType::Polygon(polygon) => self.add_polygon(
                polygon
                    .exterior()
                    .into_iter()
                    .chain(polygon.interiors())
                    .map(|ring| ring.coords().map(|c| spot(&c)).collect()),
            ),
            GeometryType::MultiPolygon(polygons) => {
                for polygon in polygons.polygons() {
                    self.add_polygon(
                        polygon
                            .exterior()
                            .into_iter()
                            .chain(polygon.interiors())
                            .map(|ring| ring.coords().map(|c| spot(&c)).collect()),
                    );
                }
            }
            GeometryType::GeometryCollection(collection) => {
                for member in collection.geometries() {
                    self.add_geometry(&member);
                }
            }
            GeometryType::Line(line) => {
                self.add_line([spot(&line.start()), spot(&line.end())].into_iter())
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
                self.add_polygon([ring].into_iter());
            }
            GeometryType::Triangle(triangle) => {
                let [a, b, c] = triangle.coords().map(|coord| spot(&coord));
                self.add_polygon([vec![a, b, c, a]].into_iter());
            }
        }
    }

    fn add_line(&mut self, spots: impl Iterator<Item = Spot>) {
        let mut previous: Option<Spot> = None;
        for current in spots {
            self.add_vector(&current.vector());
            if let Some(previous) = previous {
                self.add_edge(&previous, &current);
            }
            previous = Some(current);
        }
    }

    /// The points of the edge's great circle furthest along each axis, if they lie on it. The
    /// ends are added with the line.
    fn add_edge(&mut self, a: &Spot, b: &Spot) {
        let normal = arc_normal(a, b);
        let length = norm(&normal);
        if length == 0.0 {
            return;
        }
        let normal = normalize(&normal, length);
        for axis in 0..3 {
            let mut furthest = normal.map(|component| -normal[axis] * component);
            furthest[axis] += 1.0;
            let length = norm(&furthest);
            if length == 0.0 {
                continue;
            }
            let furthest = normalize(&furthest, length);
            for candidate in [furthest, furthest.map(|component| -component)] {
                if on_arc(&candidate, a, b, &normal) {
                    self.add_vector(&candidate);
                }
            }
        }
    }

    fn add_polygon(&mut self, rings: impl Iterator<Item = Vec<Spot>>) {
        let mut polygon = Bounds::default();
        for ring in rings {
            polygon.add_line(ring.into_iter());
        }
        let Some(mut ranges) = polygon.0 else {
            return;
        };
        let straddles = |range: (f64, f64)| range.0 < 0.0 && 0.0 < range.1;
        for axis in 0..3 {
            let (other, another) = ((axis + 1) % 3, (axis + 2) % 3);
            if straddles(ranges[other]) && straddles(ranges[another]) {
                if ranges[axis].1 > 0.0 {
                    ranges[axis].1 = 1.0;
                }
                if ranges[axis].0 < 0.0 {
                    ranges[axis].0 = -1.0;
                }
            }
        }
        self.add_bounds(&Bounds(Some(ranges)));
    }
}
