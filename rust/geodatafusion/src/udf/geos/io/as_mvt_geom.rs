use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, RectTrait};
use geodatafusion_wagyu::{Bounds, FillType};
use geos::Geom;
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::util::args::{optional_bool_arg, optional_int_arg};
use crate::util::collinear::remove_collinear;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_atoms};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AsMVTGeom(geometry geom, box2d bounds, integer extent=4096, integer buffer=256,
/// boolean clip_geom=true).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Geometry, Arg::Integer, Arg::Integer],
    &[
        Arg::Geometry,
        Arg::Geometry,
        Arg::Integer,
        Arg::Integer,
        Arg::Boolean,
    ],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "bounds", "extent", "buffer", "clip_geom"])
        .expect("parameter names are valid for a user-defined signature")
});

const DEFAULT_EXTENT: i32 = 4096;
const DEFAULT_BUFFER: i32 = 256;

/// Transforms a geometry into the coordinate space of a MVT tile.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Transforms a geometry into the coordinate space of a Mapbox Vector Tile: bounds, in the geometry's coordinates, maps to the square from 0 to extent with y pointing down, and coordinates are rounded to integers. Unless clip_geom is false, the result is clipped to bounds grown by buffer tile units; as in PostGIS, a negative buffer is read as an unsigned integer, so it doesn't clip in practice. Geometries smaller than half a tile unit both ways, and parts that collapse, are dropped; collections keep their parts of the highest dimension. Polygons are clipped and made valid with Wagyu, as in PostGIS, and are always 2D. Lines and points keep Z and M, except that clipping drops M. NULL extent, buffer or clip_geom take their defaults. Requires the wagyu feature.",
    syntax_example = "ST_AsMVTGeom(geom, bounds, extent, buffer, clip_geom)",
    argument(name = "geom", description = "geometry"),
    argument(name = "bounds", description = "box2d"),
    argument(name = "extent", description = "integer, default 4096"),
    argument(name = "buffer", description = "integer, default 256"),
    argument(name = "clip_geom", description = "boolean, default true"),
    related_udf(name = "st_tileenvelope")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsMVTGeom;

impl AsMVTGeom {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsMVTGeom {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsMVTGeom {
    fn name(&self) -> &str {
        "st_asmvtgeom"
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
        Ok(as_mvt_geom_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_mvt_geom_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let bounds = geometry_array(&args, 1)?;
    let kernel = AsMVTGeomKernel {
        bounds: map_geometry(bounds.as_ref(), &BoundsKernel)?,
        extent: optional_int_arg(&args, 2, DEFAULT_EXTENT)?,
        buffer: optional_int_arg(&args, 3, DEFAULT_BUFFER)?,
        clip_geom: optional_bool_arg(&args, 4, true)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// A box: `[min_x, min_y, max_x, max_y]`.
type BoxBounds = [f64; 4];

/// Reads a `box2d`, or the bounding box of a geometry, which PostgreSQL casts to `box2d`.
struct BoundsKernel;

impl GeometryKernel for BoundsKernel {
    type Output = BoxBounds;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<BoxBounds>> {
        if let GeometryType::Rect(rect) = geom.as_type() {
            let (min, max) = (rect.min(), rect.max());
            return Ok(Some([min.x(), min.y(), max.x(), max.y()]));
        }
        let mut bounds = BoundingRect::new(false);
        bounds.add_geometry(geom);
        Ok((!bounds.is_empty())
            .then(|| [bounds.minx(), bounds.miny(), bounds.maxx(), bounds.maxy()]))
    }
}

struct AsMVTGeomKernel {
    bounds: Vec<Option<BoxBounds>>,
    extent: Int32Array,
    buffer: Int32Array,
    clip_geom: BooleanArray,
}

impl GeometryKernel for AsMVTGeomKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_AsMVTGeom isn't STRICT: a NULL geometry gives NULL (the drivers see to that), NULL
        // bounds are an error, and the other arguments take their defaults. PostGIS checks the
        // arguments in this order, before looking at the geometry.
        let Some(bounds) = self.bounds.get(row).copied().flatten() else {
            return Err(
                exec_datafusion_err!("ST_AsMVTGeom: Geometric bounds cannot be null").into(),
            );
        };
        let [min_x, min_y, max_x, max_y] = bounds;
        if max_x - min_x <= 0.0 || max_y - min_y <= 0.0 {
            return Err(
                exec_datafusion_err!("ST_AsMVTGeom: Geometric bounds are too small").into(),
            );
        }
        let extent = value_or(&self.extent, row, DEFAULT_EXTENT);
        if extent <= 0 {
            return Err(exec_datafusion_err!("ST_AsMVTGeom: Extent must be greater than 0").into());
        }
        let buffer = value_or(&self.buffer, row, DEFAULT_BUFFER);
        let clip_geom = self.clip_geom.is_null(row) || self.clip_geom.value(row);
        Tile::new(bounds, extent, buffer, clip_geom).geometry(geom)
    }
}

fn value_or(array: &Int32Array, row: usize, default: i32) -> i32 {
    if array.is_null(row) {
        default
    } else {
        array.value(row)
    }
}

/// A closed polygon ring in tile coordinates, 2D.
type Ring = Vec<[f64; 2]>;

/// The parts of a geometry of its highest dimension.
enum Parts {
    Points(Vec<Coord<f64>>),
    Lines(Vec<Vec<Coord<f64>>>),
    Polygons(Vec<Vec<Vec<Coord<f64>>>>),
}

/// The transform into a tile, and the box to clip to there.
///
/// PostGIS's steps, as its output shows them:
///
/// 1. Collections keep their parts of the highest dimension. A line or polygon whose bounding
///    box (a polygon's is its shell's) is smaller than half a tile unit both ways is dropped.
/// 2. Coordinates are transformed into the tile and rounded to integers, halfway values to
///    even. Consecutive repeats go, then collinear points; lines and rings that collapse go, and
///    so do polygons whose shell collapses.
/// 3. Points and lines that aren't wholly inside the clip box are clipped by GEOS, which keeps Z
///    and drops M; clipped lines are rounded and cleaned again.
/// 4. Polygon rings are clipped one by one in double precision, truncated to 32-bit integers,
///    and unioned by Wagyu under the even-odd rule, which also makes them valid.
struct Tile {
    /// The affine transform into the tile: `x * x_scale + x_offset`, and the same for y, whose
    /// scale is negative because tile y points down. PostGIS applies it in one step, which
    /// rounds differently from translating and then scaling.
    x_scale: f64,
    x_offset: f64,
    y_scale: f64,
    y_offset: f64,
    /// The box to clip to, in tile coordinates, if clipping.
    clip: Option<BoxBounds>,
}

impl Tile {
    fn new(bounds: BoxBounds, extent: i32, buffer: i32, clip_geom: bool) -> Self {
        let [min_x, min_y, max_x, max_y] = bounds;
        let extent = f64::from(extent);
        // PostGIS reads the buffer as an unsigned integer: -1 is 4294967295 tile units.
        let clip = clip_geom.then(|| {
            let buffer = f64::from(buffer.cast_unsigned());
            [-buffer, -buffer, extent + buffer, extent + buffer]
        });
        let x_scale = extent / (max_x - min_x);
        let y_scale = -extent / (max_y - min_y);
        Self {
            x_scale,
            x_offset: -min_x * x_scale,
            y_scale,
            y_offset: -max_y * y_scale,
            clip,
        }
    }

    fn geometry(
        &self,
        geom: &impl GeometryTrait<T = f64>,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let atoms = owned_atoms(geom);
        let Some(parts) = highest_dimension(&atoms) else {
            return Ok(None);
        };
        // The size check looks at the whole geometry, parts of lower dimension included.
        if !matches!(parts, Parts::Points(_))
            && let Some([min_x, min_y, max_x, max_y]) = parts_bounds(&atoms)
            && (max_x - min_x) * self.x_scale < 0.5
            && (max_y - min_y) * -self.y_scale < 0.5
        {
            return Ok(None);
        }
        let dim = dimension(geom.dim());
        match parts {
            Parts::Points(points) => self.points(&points, dim),
            Parts::Lines(lines) => self.lines(&lines, dim),
            Parts::Polygons(polygons) => self.polygons(&polygons),
        }
    }

    /// The coordinate in the tile, rounded as C's `rint` does. Z and M are kept.
    fn snap(&self, c: &Coord<f64>) -> Coord<f64> {
        Coord {
            x: (c.x * self.x_scale + self.x_offset).round_ties_even(),
            y: (c.y * self.y_scale + self.y_offset).round_ties_even(),
            z: c.z,
            m: c.m,
        }
    }

    /// Whether some of the coordinates are outside the clip box, so GEOS has to clip them.
    fn needs_clipping<'a>(&self, mut coords: impl Iterator<Item = &'a Coord<f64>>) -> bool {
        let Some([min_x, min_y, max_x, max_y]) = self.clip else {
            return false;
        };
        !coords.all(|c| c.x >= min_x && c.x <= max_x && c.y >= min_y && c.y <= max_y)
    }

    /// The parts of the geometry clipped to the clip box by GEOS, and their dimension.
    fn clip_with_geos(
        &self,
        geometry: &Wkt<f64>,
    ) -> GeoDataFusionResult<(Vec<Wkt<f64>>, Dimension)> {
        let [min_x, min_y, max_x, max_y] = self.clip.expect("only called when clipping");
        let clipped = to_geos(geometry)?.clip_by_rect(min_x, min_y, max_x, max_y)?;
        let clipped = from_geos(&clipped, true)?;
        Ok((owned_atoms(&clipped), clipped.dimension()))
    }

    fn points(
        &self,
        points: &[Coord<f64>],
        mut dim: Dimension,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let mut points: Vec<Coord<f64>> = points.iter().map(|c| self.snap(c)).collect();
        if self.needs_clipping(points.iter()) {
            let geometry = Wkt::MultiPoint(MultiPoint::new(
                points.iter().map(|c| Point::new(Some(*c), dim)).collect(),
                dim,
            ));
            let (atoms, clipped_dim) = self.clip_with_geos(&geometry)?;
            dim = clipped_dim;
            points = atoms
                .iter()
                .filter_map(|atom| match atom {
                    Wkt::Point(point) => point.coord().copied(),
                    _ => None,
                })
                .collect();
        }
        // PostGIS sorts the points and drops repeats in 2D, keeping the first.
        points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        points.dedup_by(|a, b| a.x == b.x && a.y == b.y);
        let mut points: Vec<Point<f64>> = points
            .into_iter()
            .map(|c| Point::new(Some(c), dim))
            .collect();
        Ok(match points.len() {
            0 => None,
            1 => Some(Wkt::Point(points.remove(0))),
            _ => Some(Wkt::MultiPoint(MultiPoint::new(points, dim))),
        })
    }

    fn lines(
        &self,
        lines: &[Vec<Coord<f64>>],
        mut dim: Dimension,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let mut lines: Vec<Vec<Coord<f64>>> = lines
            .iter()
            .filter_map(|line| clean_line(line.iter().map(|c| self.snap(c)).collect()))
            .collect();
        if self.needs_clipping(lines.iter().flatten()) {
            let geometry = Wkt::MultiLineString(MultiLineString::new(
                lines
                    .iter()
                    .map(|line| LineString::new(line.clone(), dim))
                    .collect(),
                dim,
            ));
            let (atoms, clipped_dim) = self.clip_with_geos(&geometry)?;
            dim = clipped_dim;
            // GEOS's intersections aren't on the grid.
            let round = |c: &Coord<f64>| Coord {
                x: c.x.round_ties_even(),
                y: c.y.round_ties_even(),
                ..*c
            };
            lines = atoms
                .iter()
                .filter_map(|atom| match atom {
                    Wkt::LineString(line) => clean_line(line.coords().iter().map(round).collect()),
                    _ => None,
                })
                .collect();
        }
        let mut lines: Vec<LineString<f64>> = lines
            .into_iter()
            .map(|line| LineString::new(line, dim))
            .collect();
        Ok(match lines.len() {
            0 => None,
            1 => Some(Wkt::LineString(lines.remove(0))),
            _ => Some(Wkt::MultiLineString(MultiLineString::new(lines, dim))),
        })
    }

    fn polygons(&self, polygons: &[Vec<Vec<Coord<f64>>>]) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let polygons: Vec<Vec<Ring>> = polygons
            .iter()
            .filter_map(|rings| {
                let mut rings = rings
                    .iter()
                    .map(|ring| clean_ring(ring.iter().map(|c| self.snap(c)).collect()));
                let shell = rings.next().flatten()?;
                Some(std::iter::once(shell).chain(rings.flatten()).collect())
            })
            .collect();
        // Without clipping, PostGIS still clips to the geometry's bounding box, which cuts off
        // holes that reach out of their shells.
        let Some(clip) = self.clip.or_else(|| shell_bounds(&polygons)) else {
            return Ok(None);
        };
        let mut rings: Vec<geodatafusion_wagyu::Ring> = vec![];
        for polygon in &polygons {
            let mut clipped = polygon.iter().map(|ring| clip_ring(ring, clip));
            // A polygon whose shell is clipped away goes, holes and all.
            let Some(shell) = clipped.next().flatten() else {
                continue;
            };
            for ring in std::iter::once(shell).chain(clipped.flatten()) {
                rings.push(ring.iter().map(|&[x, y]| [to_i32(x), to_i32(y)]).collect());
            }
        }
        // The rings are clipped already, so Wagyu gets a box that clips nothing.
        let everywhere = Bounds {
            min_x: i32::MIN,
            min_y: i32::MIN,
            max_x: i32::MAX,
            max_y: i32::MAX,
        };
        let result = geodatafusion_wagyu::clip(&rings, everywhere, FillType::EvenOdd)
            .map_err(|e| exec_datafusion_err!("ST_AsMVTGeom: {e}"))?;
        let mut polygons: Vec<Polygon<f64>> = result
            .into_iter()
            .map(|rings| {
                let rings = rings
                    .into_iter()
                    .map(|ring| {
                        let coords = ring
                            .into_iter()
                            .map(|[x, y]| Coord {
                                x: f64::from(x),
                                y: f64::from(y),
                                z: None,
                                m: None,
                            })
                            .collect();
                        LineString::new(coords, Dimension::XY)
                    })
                    .collect();
                Polygon::new(rings, Dimension::XY)
            })
            .collect();
        Ok(match polygons.len() {
            0 => None,
            1 => Some(Wkt::Polygon(polygons.remove(0))),
            _ => Some(Wkt::MultiPolygon(MultiPolygon::new(
                polygons,
                Dimension::XY,
            ))),
        })
    }
}

/// The points, lines or polygons of the geometry's highest dimension, without empty parts, or
/// `None` if the geometry has no parts.
fn highest_dimension(atoms: &[Wkt<f64>]) -> Option<Parts> {
    let rank = |atom: &Wkt<f64>| match atom {
        Wkt::Point(_) => 0,
        Wkt::LineString(_) => 1,
        _ => 2,
    };
    let highest = atoms.iter().map(rank).max()?;
    let parts = atoms.iter().filter(|atom| rank(atom) == highest);
    Some(match highest {
        0 => Parts::Points(
            parts
                .filter_map(|atom| match atom {
                    Wkt::Point(point) => point.coord().copied(),
                    _ => None,
                })
                .collect(),
        ),
        1 => Parts::Lines(
            parts
                .filter_map(|atom| match atom {
                    Wkt::LineString(line) if !line.coords().is_empty() => {
                        Some(line.coords().to_vec())
                    }
                    _ => None,
                })
                .collect(),
        ),
        _ => Parts::Polygons(
            parts
                .filter_map(|atom| match atom {
                    Wkt::Polygon(polygon) if !polygon.rings().is_empty() => Some(
                        polygon
                            .rings()
                            .iter()
                            .map(|ring| ring.coords().to_vec())
                            .collect(),
                    ),
                    _ => None,
                })
                .collect(),
        ),
    })
}

/// The bounding box of the parts as PostGIS computes it: a polygon's is its shell's.
fn parts_bounds(atoms: &[Wkt<f64>]) -> Option<BoxBounds> {
    let coords = atoms.iter().flat_map(|atom| match atom {
        Wkt::Point(point) => point.coord().into_iter().collect(),
        Wkt::LineString(line) => line.coords().iter().collect(),
        Wkt::Polygon(polygon) => polygon
            .rings()
            .first()
            .map(|shell| shell.coords().iter().collect())
            .unwrap_or_default(),
        _ => vec![],
    });
    bounds(coords.map(|c| [c.x, c.y]))
}

/// The bounding box of the polygons' shells.
fn shell_bounds(polygons: &[Vec<Ring>]) -> Option<BoxBounds> {
    bounds(
        polygons
            .iter()
            .filter_map(|rings| rings.first())
            .flatten()
            .copied(),
    )
}

fn bounds(points: impl Iterator<Item = [f64; 2]>) -> Option<BoxBounds> {
    points.fold(None, |bounds, [x, y]| {
        Some(match bounds {
            None => [x, y, x, y],
            Some([min_x, min_y, max_x, max_y]) => {
                [min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y)]
            }
        })
    })
}

/// The line without consecutive repeats (comparing every ordinate, as ST_SnapToGrid does) and
/// then without collinear points, or `None` if it collapsed.
fn clean_line(mut coords: Vec<Coord<f64>>) -> Option<Vec<Coord<f64>>> {
    coords.dedup();
    if coords.len() < 2 {
        return None;
    }
    let coords = remove_collinear(&coords);
    match coords.as_slice() {
        [a, b] if a.x == b.x && a.y == b.y => None,
        _ => Some(coords),
    }
}

/// The ring cleaned as [`clean_line`] cleans a line, or `None` if fewer than four points are
/// left.
fn clean_ring(mut coords: Vec<Coord<f64>>) -> Option<Ring> {
    coords.dedup();
    if coords.len() < 4 {
        return None;
    }
    let coords = remove_collinear(&coords);
    (coords.len() >= 4).then(|| coords.iter().map(|c| [c.x, c.y]).collect())
}

/// The ring clipped to the box edge by edge (Sutherland-Hodgman, edges inside) and closed, or
/// `None` if fewer than four points are left.
///
/// PostGIS's results fit clipping in double precision by the left, right, bottom and top edges,
/// in that order: the order shows in how the intersections round.
fn clip_ring(ring: &[[f64; 2]], [min_x, min_y, max_x, max_y]: BoxBounds) -> Option<Ring> {
    let mut out = ring.to_vec();
    for edge in [Edge::MinX, Edge::MaxX, Edge::MinY, Edge::MaxY] {
        let Some(&last) = out.last() else {
            break;
        };
        let inside = |[x, y]: [f64; 2]| match edge {
            Edge::MinX => x >= min_x,
            Edge::MaxX => x <= max_x,
            Edge::MinY => y >= min_y,
            Edge::MaxY => y <= max_y,
        };
        let intersection = |[ax, ay]: [f64; 2], [bx, by]: [f64; 2]| match edge {
            Edge::MinX => [min_x, ay + (by - ay) * (min_x - ax) / (bx - ax)],
            Edge::MaxX => [max_x, ay + (by - ay) * (max_x - ax) / (bx - ax)],
            Edge::MinY => [ax + (bx - ax) * (min_y - ay) / (by - ay), min_y],
            Edge::MaxY => [ax + (bx - ax) * (max_y - ay) / (by - ay), max_y],
        };
        let input = std::mem::take(&mut out);
        let mut start = last;
        for &end in &input {
            match (inside(start), inside(end)) {
                (true, true) => out.push(end),
                (false, true) => out.extend([intersection(start, end), end]),
                (true, false) => out.push(intersection(start, end)),
                (false, false) => {}
            }
            start = end;
        }
    }
    if let Some(&first) = out.first()
        && out.last() != Some(&first)
    {
        out.push(first);
    }
    (out.len() >= 4).then_some(out)
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    MinX,
    MaxX,
    MinY,
    MaxY,
}

/// A double converted to a 32-bit integer as a C cast does on x86-64, the parity oracle's
/// platform: truncated, and `INT32_MIN` when out of range.
fn to_i32(value: f64) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::bounding_box::MakeBox2D;
    use crate::udf::native::constructors::Point as MakePoint;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_as_mvt_geom_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(AsMVTGeom.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(MakeBox2D.into());
        ctx.register_udf(MakePoint.into());

        let sql = "SELECT ST_AsMVTGeom(ST_GeomFromText('POLYGON((1 1, 3 1, 3 3, 1 1))', 3857), \
                   ST_MakeBox2D(ST_Point(0, 0), ST_Point(10, 10)), 10, 0)";
        assert_wkb_output(&ctx, sql, 3857).await;
    }
}
