use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{Coord, LineString, MultiLineString, MultiPolygon, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::util::args::optional_bool_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_RemoveIrrelevantPointsForView(geometry geom, box2d bounds, boolean
/// cartesian_hint = false). PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Removes points that are irrelevant for rendering a specific rectangular view of a geometry.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry without the points of its lines and polygon rings that are not needed to draw it inside the rectangle bounds. A point is removed if it, the last point kept and the next point all lie beyond the same side of the rectangle (for the first point of a line, the point itself counts as the last kept, and for the last point of a line, as the next). With cartesian_hint, a point outside the rectangle is also removed if the triangle it makes with those two points stays outside: none of its sides reaches the rectangle, and the rectangle's lower left corner is not in it. As in PostGIS, a side reaches the rectangle if it has an end in it or crosses or touches one of its edges, not counting the edges' upper and right ends or a side along an edge. Lines left with fewer than 2 points, rings with fewer than 4, lines and rings whose box no longer touches the rectangle, and with cartesian_hint rings that neither reach the rectangle nor go around its lower left corner, are removed, and a polygon whose exterior ring is removed is removed too. No new points are added, so the result can self-intersect. Geometries whose box lies inside the rectangle, points, curves and GEOMETRYCOLLECTIONs are returned unchanged. Returns NULL if bounds is empty.",
    syntax_example = "ST_RemoveIrrelevantPointsForView(geom, bounds, cartesian_hint)",
    argument(name = "geom", description = "geometry"),
    argument(name = "bounds", description = "box2d"),
    argument(name = "cartesian_hint", description = "boolean, default false"),
    related_udf(name = "st_clipbybox2d"),
    related_udf(name = "st_removesmallparts")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RemoveIrrelevantPointsForView;

impl RemoveIrrelevantPointsForView {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RemoveIrrelevantPointsForView {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RemoveIrrelevantPointsForView {
    fn name(&self) -> &str {
        "st_removeirrelevantpointsforview"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(remove_irrelevant_points_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn remove_irrelevant_points_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = RemoveIrrelevantPointsKernel {
        bounds: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        cartesian: optional_bool_arg(&args, 2, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct RemoveIrrelevantPointsKernel {
    bounds: OwnedColumn,
    cartesian: BooleanArray,
}

impl GeometryKernel for RemoveIrrelevantPointsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(bounds) = self.bounds.get(row) else {
            return Ok(None);
        };
        if self.cartesian.is_null(row) {
            return Ok(None);
        }
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(bounds);
        if rect.is_empty() {
            return Ok(None);
        }
        let view = View {
            xmin: rect.minx(),
            ymin: rect.miny(),
            xmax: rect.maxx(),
            ymax: rect.maxy(),
            cartesian: self.cartesian.value(row),
        };

        let geom = to_owned_geometry(geom);
        // A geometry inside the rectangle is left alone. PostGIS's box of a polygon is its
        // exterior ring's, so holes outside it don't count.
        let outline: Vec<&Coord<f64>> = match &geom {
            Wkt::LineString(line) => line.coords().iter().collect(),
            Wkt::Polygon(polygon) => polygon
                .rings()
                .iter()
                .take(1)
                .flat_map(|ring| ring.coords())
                .collect(),
            Wkt::MultiLineString(lines) => lines
                .line_strings()
                .iter()
                .flat_map(|line| line.coords())
                .collect(),
            Wkt::MultiPolygon(polygons) => polygons
                .polygons()
                .iter()
                .flat_map(|polygon| polygon.rings().iter().take(1))
                .flat_map(|ring| ring.coords())
                .collect(),
            _ => return Ok(Some(geom)),
        };
        if !outline.is_empty() && outline.iter().all(|coord| view.contains(coord)) {
            return Ok(Some(geom));
        }

        let line = |line: LineString<f64>| {
            let (coords, dim) = line.into_inner();
            view.thin(&coords, false)
                .map(|coords| LineString::new(coords, dim))
        };
        let polygon = |polygon: Polygon<f64>| {
            let (rings, dim) = polygon.into_inner();
            let mut rings = rings.into_iter();
            let exterior = view.thin(rings.next()?.coords(), true)?;
            Some(Polygon::new(
                std::iter::once(LineString::new(exterior, dim))
                    .chain(rings.filter_map(|ring| {
                        view.thin(ring.coords(), true)
                            .map(|coords| LineString::new(coords, dim))
                    }))
                    .collect(),
                dim,
            ))
        };
        Ok(Some(match geom {
            Wkt::LineString(l) => {
                let dim = l.dimension();
                Wkt::LineString(line(l).unwrap_or(LineString::new(vec![], dim)))
            }
            Wkt::Polygon(p) => {
                let dim = p.dimension();
                Wkt::Polygon(polygon(p).unwrap_or(Polygon::new(vec![], dim)))
            }
            Wkt::MultiLineString(lines) => {
                let (lines, dim) = lines.into_inner();
                Wkt::MultiLineString(MultiLineString::new(
                    lines.into_iter().filter_map(line).collect(),
                    dim,
                ))
            }
            Wkt::MultiPolygon(polygons) => {
                let (polygons, dim) = polygons.into_inner();
                Wkt::MultiPolygon(MultiPolygon::new(
                    polygons.into_iter().filter_map(polygon).collect(),
                    dim,
                ))
            }
            other => other,
        }))
    }
}

/// The rectangle to draw, and whether to use the cartesian test too.
struct View {
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    cartesian: bool,
}

impl View {
    /// The points of a line or closed ring that are kept, or `None` if it is removed.
    fn thin(&self, coords: &[Coord<f64>], ring: bool) -> Option<Vec<Coord<f64>>> {
        let mut kept: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
        if ring {
            // The closing point is left out and put back on the first point kept. The point
            // before the first is the last one before the closing point, and the point after
            // the last is the first one kept.
            let vertices = &coords[..coords.len().saturating_sub(1)];
            let mut prev = *vertices.last()?;
            for (i, &point) in vertices.iter().enumerate() {
                let next = vertices.get(i + 1).or(kept.first()).unwrap_or(&vertices[0]);
                if !self.removable(&prev, &point, next) {
                    kept.push(point);
                    prev = point;
                }
            }
            if kept.len() < 3 {
                return None;
            }
            kept.push(kept[0]);
        } else {
            // The first point stands for the last kept until a point is kept, even if it is
            // removed itself.
            let mut prev = *coords.first()?;
            for (i, &point) in coords.iter().enumerate() {
                let next = coords.get(i + 1).unwrap_or(&point);
                if !self.removable(&prev, &point, next) {
                    kept.push(point);
                    prev = point;
                }
            }
            if kept.len() < 2 {
                return None;
            }
        }
        let touches = kept.iter().any(|c| c.x <= self.xmax)
            && kept.iter().any(|c| c.x >= self.xmin)
            && kept.iter().any(|c| c.y <= self.ymax)
            && kept.iter().any(|c| c.y >= self.ymin);
        if !touches {
            return None;
        }
        // With the cartesian test, a ring must still reach the rectangle or go around it.
        if ring
            && self.cartesian
            && !kept.windows(2).any(|pair| self.reaches(&pair[0], &pair[1]))
            && !in_ring(self.xmin, self.ymin, &kept)
        {
            return None;
        }
        Some(kept)
    }

    /// Whether `point`, between the last point kept `prev` and the next point `next`, can go.
    fn removable(&self, prev: &Coord<f64>, point: &Coord<f64>, next: &Coord<f64>) -> bool {
        let beyond = [
            |view: &Self, c: &Coord<f64>| c.x < view.xmin,
            |view: &Self, c: &Coord<f64>| c.x > view.xmax,
            |view: &Self, c: &Coord<f64>| c.y < view.ymin,
            |view: &Self, c: &Coord<f64>| c.y > view.ymax,
        ];
        if beyond
            .iter()
            .any(|beyond| beyond(self, prev) && beyond(self, point) && beyond(self, next))
        {
            return true;
        }
        self.cartesian
            && !self.contains(point)
            && !self.reaches(prev, next)
            && !self.reaches(prev, point)
            && !self.reaches(point, next)
            && !in_triangle(
                &Coord {
                    x: self.xmin,
                    y: self.ymin,
                    z: None,
                    m: None,
                },
                prev,
                point,
                next,
            )
    }

    /// Whether `c` is in the rectangle, edges included.
    fn contains(&self, c: &Coord<f64>) -> bool {
        self.xmin <= c.x && c.x <= self.xmax && self.ymin <= c.y && c.y <= self.ymax
    }

    /// Whether the segment `a`-`b` reaches the rectangle: it has an end in it, or crosses or
    /// touches one of its edges, an edge's upper or right end excluded. A segment along an edge
    /// doesn't count for that edge, so one along the top or right edge, or touching only the
    /// upper right corner, doesn't reach the rectangle.
    fn reaches(&self, a: &Coord<f64>, b: &Coord<f64>) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let corner = |x, y| Coord {
            x,
            y,
            z: None,
            m: None,
        };
        let (lower_left, upper_right) =
            (corner(self.xmin, self.ymin), corner(self.xmax, self.ymax));
        let edges = [
            (lower_left, corner(self.xmax, self.ymin)),
            (corner(self.xmin, self.ymax), upper_right),
            (lower_left, corner(self.xmin, self.ymax)),
            (corner(self.xmax, self.ymin), upper_right),
        ];
        edges.iter().any(|(start, end)| {
            // The edge's start may lie on the segment's line, its end may not.
            let (at_start, at_end) = (orientation(a, b, start), orientation(a, b, end));
            let (at_a, at_b) = (orientation(start, end, a), orientation(start, end, b));
            at_end != 0.0
                && (at_start == 0.0 || (at_start > 0.0) != (at_end > 0.0))
                && !(at_a > 0.0 && at_b > 0.0 || at_a < 0.0 && at_b < 0.0)
        })
    }
}

/// Twice the signed area of the triangle `p`, `q`, `r`: positive if it turns left.
fn orientation(p: &Coord<f64>, q: &Coord<f64>, r: &Coord<f64>) -> f64 {
    (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)
}

/// Whether `q` is in the triangle `a`, `b`, `c`, edges included.
fn in_triangle(q: &Coord<f64>, a: &Coord<f64>, b: &Coord<f64>, c: &Coord<f64>) -> bool {
    let (o1, o2, o3) = (
        orientation(a, b, q),
        orientation(b, c, q),
        orientation(c, a, q),
    );
    (o1 >= 0.0 && o2 >= 0.0 && o3 >= 0.0) || (o1 <= 0.0 && o2 <= 0.0 && o3 <= 0.0)
}

/// Whether the point (`x`, `y`) is inside the closed `ring`, by the number of its edges that
/// meet the horizontal ray to the left of the point. As in PostGIS, an edge meeting the ray at an
/// end counts, so a vertex on the ray counts twice.
fn in_ring(x: f64, y: f64, ring: &[Coord<f64>]) -> bool {
    let mut inside = false;
    for pair in ring.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let dy = b.y - a.y;
        if dy == 0.0 {
            continue;
        }
        let t = (y - a.y) / dy;
        if (0.0..=1.0).contains(&t) && a.x + t * (b.x - a.x) < x {
            inside = !inside;
        }
    }
    inside
}
