//! ST_SimplifyVW.

use std::cell::Cell;
use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
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
use wkt::types::{Coord, GeometryCollection, LineString, MultiLineString, MultiPolygon, Polygon};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_SimplifyVW(geometry geom, float tolerance).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "tolerance"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a simplified representation of a geometry, using the Visvalingam-Whyatt algorithm.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a simplified representation of a geometry using the Visvalingam-Whyatt algorithm. The simplification tolerance is an area value, in the units of the input SRS. Simplification removes vertices which form \"corners\" with area less than the tolerance. The result may not be valid even if the input is. Lines keep their ends and polygon shells at least four points; holes left with fewer than four points are removed. Points and empty parts are kept. Z and M are kept.",
    syntax_example = "ST_SimplifyVW(geom, tolerance)",
    argument(name = "geom", description = "geometry"),
    argument(name = "tolerance", description = "float")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SimplifyVW;

impl SimplifyVW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimplifyVW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SimplifyVW {
    fn name(&self) -> &str {
        "st_simplifyvw"
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
        Ok(simplify_vw_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn simplify_vw_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = SimplifyVWKernel {
        tolerance: optional_float_arg(&args, 1, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SimplifyVWKernel {
    tolerance: Float64Array,
}

impl GeometryKernel for SimplifyVWKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_SimplifyVW is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.tolerance.is_null(row) {
            return Ok(None);
        }
        let simplifier = Simplifier {
            tolerance: self.tolerance.value(row),
            removed: Cell::new(false),
        };
        let input = to_owned_geometry(geom);
        let result = simplifier.geometry(&input);
        // If no vertex was removed, PostGIS returns the input unchanged.
        Ok(Some(if simplifier.removed.get() {
            result
        } else {
            input
        }))
    }
}

struct Simplifier {
    tolerance: f64,
    /// Whether a vertex was removed so far.
    removed: Cell<bool>,
}

impl Simplifier {
    /// The simplified geometry. Unlike ST_Simplify, nothing collapses but holes, so EMPTY parts
    /// are kept.
    fn geometry(&self, geom: &Wkt<f64>) -> Wkt<f64> {
        match geom {
            Wkt::Point(_) | Wkt::MultiPoint(_) => geom.clone(),
            Wkt::LineString(line) => Wkt::LineString(self.line(line)),
            Wkt::Polygon(polygon) => Wkt::Polygon(self.polygon(polygon)),
            Wkt::MultiLineString(lines) => Wkt::MultiLineString(MultiLineString::new(
                lines
                    .line_strings()
                    .iter()
                    .map(|line| self.line(line))
                    .collect(),
                geom.dimension(),
            )),
            Wkt::MultiPolygon(polygons) => Wkt::MultiPolygon(MultiPolygon::new(
                polygons
                    .polygons()
                    .iter()
                    .map(|polygon| self.polygon(polygon))
                    .collect(),
                geom.dimension(),
            )),
            Wkt::GeometryCollection(collection) => {
                Wkt::GeometryCollection(GeometryCollection::new(
                    collection
                        .geometries()
                        .iter()
                        .map(|member| self.geometry(member))
                        .collect(),
                    geom.dimension(),
                ))
            }
        }
    }

    /// A line keeps its ends, so it never collapses: two identical points are kept too.
    fn line(&self, line: &LineString<f64>) -> LineString<f64> {
        LineString::new(self.coords(line.coords(), 2), line.dimension())
    }

    /// The shell keeps at least four points. A hole has no minimum, and is removed if fewer
    /// than four are left.
    fn polygon(&self, polygon: &Polygon<f64>) -> Polygon<f64> {
        let Some((shell, holes)) = polygon.rings().split_first() else {
            return polygon.clone();
        };
        let rings = std::iter::once(self.coords(shell.coords(), 4))
            .chain(
                holes
                    .iter()
                    .map(|hole| self.coords(hole.coords(), 0))
                    .filter(|hole| hole.len() >= 4),
            )
            .map(|ring| LineString::new(ring, polygon.dimension()))
            .collect();
        Polygon::new(rings, polygon.dimension())
    }

    fn coords(&self, coords: &[Coord<f64>], min_points: usize) -> Vec<Coord<f64>> {
        let kept = visvalingam_whyatt(coords, self.tolerance, min_points);
        if kept.len() < coords.len() {
            self.removed.set(true);
        }
        kept
    }
}

/// The coordinates Visvalingam-Whyatt keeps: it removes the vertex with the smallest effective
/// area until that area is at least `tolerance` or only `min_points` are left. The ends are
/// never removed.
///
/// A vertex's area is that of the triangle with its neighbours; its effective area is that,
/// or the effective area of the last vertex removed if that is larger. Of vertices with equal
/// effective areas, the order the heap gives them in decides. The heap is built and updated as
/// PostGIS's evidently is, derived from its output: the vertices start sorted by area, and a
/// removal (but not an update) moves the last vertex of the heap past equal ones.
fn visvalingam_whyatt(coords: &[Coord<f64>], tolerance: f64, min_points: usize) -> Vec<Coord<f64>> {
    let n = coords.len();
    if n < 3 {
        return coords.to_vec();
    }
    let area = |previous: usize, index: usize, next: usize| {
        let (a, b, c) = (&coords[previous], &coords[index], &coords[next]);
        ((a.x - b.x) * (c.y - b.y) - (c.x - b.x) * (a.y - b.y)).abs() / 2.0
    };
    // Neighbours in the line as vertices are removed.
    let mut previous: Vec<usize> = (0..n).map(|index| index.saturating_sub(1)).collect();
    let mut next: Vec<usize> = (1..=n).collect();
    // The ends are never removed; their keys are never read.
    let mut keys: Vec<f64> = (0..n)
        .map(|index| match index {
            0 => f64::INFINITY,
            _ if index == n - 1 => f64::INFINITY,
            _ => area(index - 1, index, index + 1),
        })
        .collect();
    let mut order: Vec<usize> = (1..n - 1).collect();
    order.sort_by(|&a, &b| keys[a].total_cmp(&keys[b]));
    let mut heap = Heap::new(order, n);

    let mut removed = vec![false; n];
    let mut alive = n;
    let mut last = 0.0_f64;
    while let Some(index) = heap.peek() {
        if keys[index] >= tolerance || alive <= min_points {
            break;
        }
        heap.pop(&keys);
        removed[index] = true;
        alive -= 1;
        last = last.max(keys[index]);
        let (before, after) = (previous[index], next[index]);
        next[before] = after;
        previous[after] = before;
        for neighbour in [before, after] {
            if neighbour != 0 && neighbour != n - 1 {
                keys[neighbour] = area(previous[neighbour], neighbour, next[neighbour]).max(last);
                heap.update(neighbour, &keys);
            }
        }
    }
    coords
        .iter()
        .zip(removed)
        .filter_map(|(coord, removed)| (!removed).then_some(*coord))
        .collect()
}

/// A binary min-heap of vertex indices, keyed by a slice of effective areas.
struct Heap {
    items: Vec<usize>,
    /// The position in `items` of each vertex.
    positions: Vec<usize>,
}

impl Heap {
    /// A heap from vertices sorted by key, which is already a heap.
    fn new(sorted: Vec<usize>, vertices: usize) -> Self {
        let mut positions = vec![usize::MAX; vertices];
        for (position, &item) in sorted.iter().enumerate() {
            positions[item] = position;
        }
        Self {
            items: sorted,
            positions,
        }
    }

    fn peek(&self) -> Option<usize> {
        self.items.first().copied()
    }

    fn pop(&mut self, keys: &[f64]) {
        let last = self
            .items
            .pop()
            .expect("pop is only called on a non-empty heap");
        if !self.items.is_empty() {
            self.items[0] = last;
            self.positions[last] = 0;
            self.sift_down(0, keys, true);
        }
    }

    fn update(&mut self, item: usize, keys: &[f64]) {
        let position = self.sift_up(self.positions[item], keys);
        self.sift_down(position, keys, false);
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
        self.positions[self.items[a]] = a;
        self.positions[self.items[b]] = b;
    }

    fn sift_up(&mut self, mut position: usize, keys: &[f64]) -> usize {
        while position > 0 {
            let parent = (position - 1) / 2;
            if keys[self.items[position]] >= keys[self.items[parent]] {
                break;
            }
            self.swap(position, parent);
            position = parent;
        }
        position
    }

    /// Moves the item at `position` below smaller children, and below equal ones if
    /// `past_equal`.
    fn sift_down(&mut self, mut position: usize, keys: &[f64], past_equal: bool) {
        loop {
            let (left, right) = (2 * position + 1, 2 * position + 2);
            if left >= self.items.len() {
                break;
            }
            let child =
                if right < self.items.len() && keys[self.items[right]] < keys[self.items[left]] {
                    right
                } else {
                    left
                };
            let (child_key, key) = (keys[self.items[child]], keys[self.items[position]]);
            if child_key < key || (past_equal && child_key == key) {
                self.swap(position, child);
                position = child;
            } else {
                break;
            }
        }
    }
}
