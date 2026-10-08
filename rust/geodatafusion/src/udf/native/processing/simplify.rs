//! ST_Simplify.

use std::cell::Cell;
use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use wkt::Wkt;
use wkt::types::{
    Coord, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::args::{optional_bool_arg, optional_float_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Simplify(geometry geom, float tolerance) and ST_Simplify(geometry geom, float
/// tolerance, boolean preserveCollapsed).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "tolerance", "preserveCollapsed"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a simplified representation of a geometry, using the Douglas-Peucker algorithm.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a simplified representation of a geometry using the Douglas-Peucker algorithm. The simplification tolerance is a distance value, in the units of the input SRS. Simplification removes vertices which are within the tolerance distance of the simplified linework. The result may not be valid even if the input is. Lines that collapse to two identical points, rings that collapse to fewer than four points and polygons whose shell collapses are removed, and NULL is returned if nothing is left. preserveCollapsed (default false) keeps them instead: lines as two points, shells as four. Points are returned unchanged, as is any geometry from which no vertex is removed. Z and M are kept.",
    syntax_example = "ST_Simplify(geom, tolerance, preserveCollapsed)",
    argument(name = "geom", description = "geometry"),
    argument(name = "tolerance", description = "float"),
    argument(name = "preserveCollapsed", description = "boolean")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Simplify;

impl Simplify {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Simplify {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Simplify {
    fn name(&self) -> &str {
        "st_simplify"
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
        Ok(simplify_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn simplify_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = SimplifyKernel {
        tolerance: optional_float_arg(&args, 1, 0.0)?,
        preserve_collapsed: optional_bool_arg(&args, 2, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SimplifyKernel {
    tolerance: Float64Array,
    preserve_collapsed: BooleanArray,
}

impl GeometryKernel for SimplifyKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_Simplify is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.tolerance.is_null(row) || self.preserve_collapsed.is_null(row) {
            return Ok(None);
        }
        let tolerance = self.tolerance.value(row);
        let simplifier = Simplifier {
            // PostGIS compares squared distances, so a negative tolerance acts as its absolute
            // value.
            tolerance_squared: tolerance * tolerance,
            tolerance_is_zero: tolerance == 0.0,
            preserve_collapsed: self.preserve_collapsed.value(row),
            removed: Cell::new(false),
        };
        let input = to_owned_geometry(geom);
        let result = simplifier.geometry(&input);
        // If no vertex was removed, PostGIS returns the input, EMPTY and collapsed parts included.
        if !simplifier.removed.get() {
            return Ok(Some(input));
        }
        Ok(result)
    }
}

struct Simplifier {
    tolerance_squared: f64,
    /// PostGIS doesn't run Douglas-Peucker for a tolerance of exactly 0, see
    /// [`remove_collinear`].
    tolerance_is_zero: bool,
    preserve_collapsed: bool,
    /// Whether a vertex was removed so far.
    removed: Cell<bool>,
}

impl Simplifier {
    /// The simplified geometry, or `None` if it collapsed. EMPTY parts count as collapsed.
    fn geometry(&self, geom: &Wkt<f64>) -> Option<Wkt<f64>> {
        match geom {
            Wkt::Point(point) => point.coord().is_some().then(|| geom.clone()),
            Wkt::MultiPoint(points) => {
                let points: Vec<_> = points
                    .points()
                    .iter()
                    .filter(|point| point.coord().is_some())
                    .cloned()
                    .collect();
                (!points.is_empty())
                    .then(|| Wkt::MultiPoint(MultiPoint::new(points, geom.dimension())))
            }
            Wkt::LineString(line) => self.line(line).map(Wkt::LineString),
            Wkt::Polygon(polygon) => self.polygon(polygon).map(Wkt::Polygon),
            Wkt::MultiLineString(lines) => {
                let lines: Vec<_> = lines
                    .line_strings()
                    .iter()
                    .filter_map(|line| self.line(line))
                    .collect();
                (!lines.is_empty())
                    .then(|| Wkt::MultiLineString(MultiLineString::new(lines, geom.dimension())))
            }
            Wkt::MultiPolygon(polygons) => {
                let polygons: Vec<_> = polygons
                    .polygons()
                    .iter()
                    .filter_map(|polygon| self.polygon(polygon))
                    .collect();
                (!polygons.is_empty())
                    .then(|| Wkt::MultiPolygon(MultiPolygon::new(polygons, geom.dimension())))
            }
            Wkt::GeometryCollection(collection) => {
                let members: Vec<_> = collection
                    .geometries()
                    .iter()
                    .filter_map(|member| self.geometry(member))
                    .collect();
                (!members.is_empty()).then(|| {
                    Wkt::GeometryCollection(GeometryCollection::new(members, geom.dimension()))
                })
            }
        }
    }

    /// A line keeps its ends; it collapsed if they are all that is left and they are the same
    /// point.
    fn line(&self, line: &LineString<f64>) -> Option<LineString<f64>> {
        if line.coords().is_empty() {
            return None;
        }
        let coords = self.coords(line.coords(), 2);
        let collapsed = matches!(coords.as_slice(), [a, b] if a.x == b.x && a.y == b.y);
        (self.preserve_collapsed || !collapsed).then(|| LineString::new(coords, line.dimension()))
    }

    /// A ring collapsed if fewer than four points are left; a polygon, if its shell collapsed.
    /// preserveCollapsed keeps four points of the shell, but not the holes.
    fn polygon(&self, polygon: &Polygon<f64>) -> Option<Polygon<f64>> {
        let (shell, holes) = polygon.rings().split_first()?;
        let shell = self.coords(shell.coords(), if self.preserve_collapsed { 4 } else { 0 });
        if shell.len() < 4 {
            return None;
        }
        let rings = std::iter::once(shell)
            .chain(
                holes
                    .iter()
                    .map(|hole| self.coords(hole.coords(), 0))
                    .filter(|hole| hole.len() >= 4),
            )
            .map(|ring| LineString::new(ring, polygon.dimension()))
            .collect();
        Some(Polygon::new(rings, polygon.dimension()))
    }

    /// The coordinates Douglas-Peucker keeps, at least `min_points` of them if there are as
    /// many. For a tolerance of 0, the coordinates [`remove_collinear`] keeps, unless they are
    /// fewer than `min_points`.
    fn coords(&self, coords: &[Coord<f64>], min_points: usize) -> Vec<Coord<f64>> {
        if coords.len() < 3 {
            return coords.to_vec();
        }
        let mut kept = None;
        if self.tolerance_is_zero {
            kept = Some(remove_collinear(coords)).filter(|kept| kept.len() >= min_points);
        }
        let kept = kept.unwrap_or_else(|| self.douglas_peucker(coords, min_points));
        if kept.len() < coords.len() {
            self.removed.set(true);
        }
        kept
    }

    fn douglas_peucker(&self, coords: &[Coord<f64>], min_points: usize) -> Vec<Coord<f64>> {
        let mut keep = vec![false; coords.len()];
        keep[0] = true;
        keep[coords.len() - 1] = true;
        let mut kept = 2;
        self.split(
            coords,
            0,
            coords.len() - 1,
            min_points,
            &mut keep,
            &mut kept,
        );
        coords
            .iter()
            .zip(keep)
            .filter_map(|(coord, keep)| keep.then_some(*coord))
            .collect()
    }

    /// Keeps the vertex between `first` and `last` farthest from the segment between them, if it
    /// is farther than the tolerance or fewer than `min_points` are kept, and recurses into
    /// both halves, the first half first. Of equally far vertices, the first is kept.
    fn split(
        &self,
        coords: &[Coord<f64>],
        first: usize,
        last: usize,
        min_points: usize,
        keep: &mut [bool],
        kept: &mut usize,
    ) {
        if last <= first + 1 {
            return;
        }
        let (a, b) = (xy(&coords[first]), xy(&coords[last]));
        let mut farthest = first + 1;
        let mut max = -1.0;
        for (index, coord) in coords.iter().enumerate().take(last).skip(first + 1) {
            let distance = segment_distance_squared(xy(coord), a, b);
            if distance > max {
                max = distance;
                farthest = index;
            }
        }
        if max > self.tolerance_squared || *kept < min_points {
            keep[farthest] = true;
            *kept += 1;
            self.split(coords, first, farthest, min_points, keep, kept);
            self.split(coords, farthest, last, min_points, keep, kept);
        }
    }
}

type Xy = (f64, f64);

fn xy(coord: &Coord<f64>) -> Xy {
    (coord.x, coord.y)
}

/// Removes, in order, every vertex on the segment from the last vertex kept to the next
/// vertex, ends included. A vertex is kept if that segment is a single point.
///
/// This is what PostGIS does for a tolerance of 0, derived from its output: unlike Douglas-Peucker
/// with a tolerance of 0, it keeps vertices that backtrack along a line, and all but one of the
/// copies of a line's first vertex.
fn remove_collinear(coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut kept = vec![coords[0]];
    for window in coords.windows(3) {
        let (a, p, b) = (
            xy(kept.last().expect("the first vertex is kept")),
            xy(&window[1]),
            xy(&window[2]),
        );
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length_squared = dx * dx + dy * dy;
        let dot = (p.0 - a.0) * dx + (p.1 - a.1) * dy;
        let cross = dx * (p.1 - a.1) - dy * (p.0 - a.0);
        let on_segment =
            length_squared != 0.0 && cross == 0.0 && (0.0..=length_squared).contains(&dot);
        if !on_segment {
            kept.push(window[1]);
        }
    }
    kept.push(coords[coords.len() - 1]);
    kept
}

/// The squared distance from `p` to the segment `a`-`b`.
///
/// The distance from the segment's line is computed from the signed area `p` spans with the
/// segment, squared before dividing by the segment's squared length. Dividing first, as
/// [`ST_Distance`](crate::udf::native::measurement::Distance) does, rounds differently, and
/// at a distance equal to the tolerance PostGIS keeps a vertex only with this order.
fn segment_distance_squared(p: Xy, a: Xy, b: Xy) -> f64 {
    let point_distance_squared = |q: Xy| (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2);
    if a == b {
        return point_distance_squared(a);
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let dot = (p.0 - a.0) * dx + (p.1 - a.1) * dy;
    if dot <= 0.0 {
        return point_distance_squared(a);
    }
    if dot >= length_squared {
        return point_distance_squared(b);
    }
    let area = (a.1 - p.1) * dx - (a.0 - p.0) * dy;
    area * area / length_squared
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_simplify_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(Simplify.into());
        ctx.register_udf(GeomFromText::default().into());

        let sql =
            "SELECT ST_Simplify(ST_GeomFromText('LINESTRING(0 0,5 4,11 5.5,27.8 0.1)', 3857), 1.0)";
        assert_wkb_output(&ctx, sql, 3857).await;
    }
}
