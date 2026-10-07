//! ST_Distance, and the planar distance ST_DWithin shares.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Distance(geometry g1, geometry g2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the distance between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "For geometry types returns the minimum 2D Cartesian (planar) distance between two geometries, in projected units (spatial ref units). A point inside a polygon is at distance 0 from it. Returns NULL if either geometry is empty. Z and M are ignored.",
    syntax_example = "ST_Distance(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance;

impl Distance {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Distance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Distance {
    fn name(&self) -> &str {
        "st_distance"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DistanceKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for DistanceKernel {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self.geom2.get(row).and_then(|geom2| distance(geom1, geom2)))
    }
}

/// The minimum planar distance between two geometries, or `None` if either is empty.
pub(crate) fn distance(
    geom1: &impl GeometryTrait<T = f64>,
    geom2: &impl GeometryTrait<T = f64>,
) -> Option<f64> {
    let (parts1, parts2) = (parts(geom1), parts(geom2));
    if parts1.is_empty() || parts2.is_empty() {
        return None;
    }
    let boxes1: Vec<Bbox> = parts1.iter().map(Part::bbox).collect();
    let boxes2: Vec<Bbox> = parts2.iter().map(Part::bbox).collect();
    let mut min = f64::INFINITY;
    for (part1, box1) in parts1.iter().zip(&boxes1) {
        for (part2, box2) in parts2.iter().zip(&boxes2) {
            // The gap between the boxes is a lower bound, so this pair can't be nearer.
            if box1.gap(box2) > min {
                continue;
            }
            min = min.min(part_distance(part1, part2));
            if min == 0.0 {
                return Some(0.0);
            }
        }
    }
    Some(min)
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

type Xy = (f64, f64);

/// A non-empty basic part of a geometry. A polygon's first ring is its shell.
enum Part {
    Point(Xy),
    Line(Vec<Xy>),
    Polygon(Vec<Vec<Xy>>),
}

impl Part {
    fn bbox(&self) -> Bbox {
        match self {
            Part::Point(p) => Bbox::of([*p]),
            Part::Line(line) => Bbox::of(line.iter().copied()),
            // The holes are inside the shell.
            Part::Polygon(rings) => Bbox::of(rings[0].iter().copied()),
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

fn part_distance(part1: &Part, part2: &Part) -> f64 {
    match (part1, part2) {
        (Part::Point(p), Part::Point(q)) => point_point(*p, *q),
        (Part::Point(p), Part::Line(line)) | (Part::Line(line), Part::Point(p)) => {
            point_line(*p, line)
        }
        (Part::Line(line1), Part::Line(line2)) => line_line(line1, line2),
        (Part::Point(p), Part::Polygon(rings)) | (Part::Polygon(rings), Part::Point(p)) => {
            match facing_ring(*p, rings) {
                None => 0.0,
                Some(ring) => point_line(*p, ring),
            }
        }
        (Part::Line(line), Part::Polygon(rings)) | (Part::Polygon(rings), Part::Line(line)) => {
            // A line that doesn't cross the ring it starts behind stays behind it.
            match facing_ring(line[0], rings) {
                None => 0.0,
                Some(ring) => line_line(line, ring),
            }
        }
        (Part::Polygon(rings1), Part::Polygon(rings2)) => {
            // Likewise for the polygons' shells.
            match (
                facing_ring(rings1[0][0], rings2),
                facing_ring(rings2[0][0], rings1),
            ) {
                (Some(ring2), Some(ring1)) => line_line(ring1, ring2),
                _ => 0.0,
            }
        }
    }
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

fn point_line(p: Xy, line: &[Xy]) -> f64 {
    match line {
        [only] => point_point(p, *only),
        _ => line
            .windows(2)
            .map(|segment| point_segment(p, segment[0], segment[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

fn line_line(line1: &[Xy], line2: &[Xy]) -> f64 {
    match (line1, line2) {
        ([p], line) | (line, [p]) => point_line(*p, line),
        _ => {
            let mut min = f64::INFINITY;
            for a in line1.windows(2) {
                let box_a = Bbox::of([a[0], a[1]]);
                for b in line2.windows(2) {
                    if box_a.gap(&Bbox::of([b[0], b[1]])) > min {
                        continue;
                    }
                    min = min.min(segment_segment(a[0], a[1], b[0], b[1]));
                    if min == 0.0 {
                        return 0.0;
                    }
                }
            }
            min
        }
    }
}

/// The distance from `p` to the segment `a`-`b`: to the nearest end if `p` projects outside
/// the segment, otherwise its distance from the segment's line.
///
/// The line distance is computed as JTS does (`Distance.pointToSegment`), from the signed area
/// `p` spans with the segment. It agrees with PostGIS to the last digit, also for points a
/// rounding error away from the line, where projecting `p` onto the line doesn't.
fn point_segment(p: Xy, a: Xy, b: Xy) -> f64 {
    if a == b {
        return point_point(p, a);
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let r = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_squared;
    if r <= 0.0 {
        return point_point(p, a);
    }
    if r >= 1.0 {
        return point_point(p, b);
    }
    let s = ((a.1 - p.1) * dx - (a.0 - p.0) * dy) / length_squared;
    s.abs() * length_squared.sqrt()
}

/// The distance between the segments `a`-`b` and `c`-`d`: 0 if they cross, otherwise the
/// smallest distance from an end of one to the other.
fn segment_segment(a: Xy, b: Xy, c: Xy, d: Xy) -> f64 {
    let (side_a, side_b) = (orientation(c, d, a), orientation(c, d, b));
    let (side_c, side_d) = (orientation(a, b, c), orientation(a, b, d));
    if side_a * side_b < 0.0 && side_c * side_d < 0.0 {
        return 0.0;
    }
    point_segment(a, c, d)
        .min(point_segment(b, c, d))
        .min(point_segment(c, a, b))
        .min(point_segment(d, a, b))
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
