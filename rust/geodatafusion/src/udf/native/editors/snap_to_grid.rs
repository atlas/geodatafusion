use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{
    Coord, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
    Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_SnapToGrid(geometry geomA, float size)
/// - ST_SnapToGrid(geometry geomA, float sizeX, float sizeY)
/// - ST_SnapToGrid(geometry geomA, float originX, float originY, float sizeX, float sizeY)
/// - ST_SnapToGrid(geometry geom1, geometry geom2, float sizeX, float sizeY, float sizeZ,
///   float sizeM)
///
/// PostGIS only names `geom1` and `geom2`.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[
        Arg::Geometry,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
    ],
    &[
        Arg::Geometry,
        Arg::Geometry,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
    ],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Snap all points of the input geometry to a regular grid.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Snaps every coordinate to a regular grid with the given cell sizes and origin (default 0), rounding halfway values to even. The forms with numbers snap X and Y; the form with an origin POINT also snaps Z and M. A size of 0 or less leaves that ordinate as it is. Consecutive repeated points are then removed, and lines with fewer than 2 points and rings with fewer than 4 are dropped, along with empty members of collections; a geometry that collapses entirely becomes empty. If no size is positive, the geometry is returned unchanged.",
    syntax_example = "ST_SnapToGrid(geomA, originX, originY, sizeX, sizeY)",
    alternative_syntax = "ST_SnapToGrid(geom1, geom2, sizeX, sizeY, sizeZ, sizeM)",
    argument(name = "geomA", description = "geometry"),
    argument(
        name = "originX",
        description = "float8: size, sizeX, originX, or for the last form geom2, a POINT origin"
    ),
    argument(name = "originY", description = "float8"),
    argument(name = "sizeX", description = "float8"),
    argument(name = "sizeY", description = "float8"),
    argument(name = "sizeM", description = "float8, last form only"),
    related_udf(name = "st_reduceprecision"),
    related_udf(name = "st_quantizecoordinates")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SnapToGrid;

impl SnapToGrid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SnapToGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SnapToGrid {
    fn name(&self) -> &str {
        "st_snaptogrid"
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
        Ok(snap_to_grid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn snap_to_grid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let float = |index| optional_float_arg(&args, index, 0.0);
    let kernel = match args.args.len() {
        2 => SnapToGridKernel {
            origin: Origin::Zero,
            sizes: [float(1)?, float(1)?],
            sizes_zm: None,
        },
        3 => SnapToGridKernel {
            origin: Origin::Zero,
            sizes: [float(1)?, float(2)?],
            sizes_zm: None,
        },
        5 => SnapToGridKernel {
            origin: Origin::Coordinates(float(1)?, float(2)?),
            sizes: [float(3)?, float(4)?],
            sizes_zm: None,
        },
        _ => SnapToGridKernel {
            origin: Origin::Point(OwnedColumn::try_new(
                &args.args[1],
                &args.arg_fields[1],
                args.number_rows,
            )?),
            sizes: [float(2)?, float(3)?],
            sizes_zm: Some([float(4)?, float(5)?]),
        },
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

enum Origin {
    Zero,
    Coordinates(Float64Array, Float64Array),
    Point(OwnedColumn),
}

/// The origin, the X and Y cell sizes, and the Z and M cell sizes of the form that has them.
struct SnapToGridKernel {
    origin: Origin,
    sizes: [Float64Array; 2],
    sizes_zm: Option<[Float64Array; 2]>,
}

impl SnapToGridKernel {
    /// The grid in row `row`; `None` if an argument is NULL.
    fn grid(&self, row: usize) -> GeoDataFusionResult<Option<Grid>> {
        let value = |array: &Float64Array| (!array.is_null(row)).then(|| array.value(row));
        let origin = match &self.origin {
            Origin::Zero => Coord {
                x: 0.0,
                y: 0.0,
                z: None,
                m: None,
            },
            Origin::Coordinates(x, y) => {
                let (Some(x), Some(y)) = (value(x), value(y)) else {
                    return Ok(None);
                };
                Coord {
                    x,
                    y,
                    z: None,
                    m: None,
                }
            }
            Origin::Point(points) => match points.get(row) {
                None => return Ok(None),
                Some(Wkt::Point(point)) => match point.coord() {
                    Some(coord) => *coord,
                    None => {
                        return Err(exec_datafusion_err!(
                            "st_snaptogrid: Offset geometry must not be empty"
                        )
                        .into());
                    }
                },
                Some(_) => {
                    return Err(exec_datafusion_err!(
                        "st_snaptogrid: Offset geometry must be a point"
                    )
                    .into());
                }
            },
        };
        let [size_x, size_y] = &self.sizes;
        let (Some(size_x), Some(size_y)) = (value(size_x), value(size_y)) else {
            return Ok(None);
        };
        let (size_z, size_m) = match &self.sizes_zm {
            None => (0.0, 0.0),
            Some([size_z, size_m]) => match (value(size_z), value(size_m)) {
                (Some(size_z), Some(size_m)) => (size_z, size_m),
                _ => return Ok(None),
            },
        };
        Ok(Some(Grid {
            origin,
            size_x,
            size_y,
            size_z,
            size_m,
        }))
    }
}

impl GeometryKernel for SnapToGridKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(grid) = self.grid(row)? else {
            return Ok(None);
        };
        let geom = to_owned_geometry(geom);
        // PostGIS returns the input as it is when there is nothing to snap.
        if !grid.snaps() {
            return Ok(Some(geom));
        }
        Ok(Some(snap_geometry(geom, &grid)))
    }
}

struct Grid {
    origin: Coord<f64>,
    size_x: f64,
    size_y: f64,
    size_z: f64,
    size_m: f64,
}

impl Grid {
    fn snaps(&self) -> bool {
        [self.size_x, self.size_y, self.size_z, self.size_m]
            .iter()
            .any(|&size| size > 0.0)
    }

    fn snap(&self, coord: Coord<f64>) -> Coord<f64> {
        // C's rint, as PostGIS uses: halfway values go to the even neighbour.
        let snap = |value: f64, origin: f64, size: f64| {
            if size > 0.0 {
                ((value - origin) / size).round_ties_even() * size + origin
            } else {
                value
            }
        };
        Coord {
            x: snap(coord.x, self.origin.x, self.size_x),
            y: snap(coord.y, self.origin.y, self.size_y),
            z: coord
                .z
                .map(|z| snap(z, self.origin.z.unwrap_or(0.0), self.size_z)),
            m: coord
                .m
                .map(|m| snap(m, self.origin.m.unwrap_or(0.0), self.size_m)),
        }
    }

    /// The snapped coordinates, without consecutive repeats.
    fn snap_coords(&self, coords: Vec<Coord<f64>>) -> Vec<Coord<f64>> {
        let mut snapped: Vec<Coord<f64>> = coords.into_iter().map(|c| self.snap(c)).collect();
        snapped.dedup();
        snapped
    }
}

/// The snapped geometry; a geometry that collapses entirely becomes an empty one of its type.
fn snap_geometry(geom: Wkt<f64>, grid: &Grid) -> Wkt<f64> {
    match geom {
        Wkt::Point(point) => Wkt::Point(snap_point(point, grid)),
        Wkt::LineString(line) => {
            let dim = line.dimension();
            Wkt::LineString(snap_line_string(line, grid).unwrap_or(LineString::new(vec![], dim)))
        }
        Wkt::Polygon(polygon) => {
            let dim = polygon.dimension();
            Wkt::Polygon(snap_polygon(polygon, grid).unwrap_or(Polygon::new(vec![], dim)))
        }
        Wkt::MultiPoint(points) => {
            let (points, dim) = points.into_inner();
            // Empty points are dropped.
            let points = points
                .into_iter()
                .map(|point| snap_point(point, grid))
                .filter(|point| point.coord().is_some())
                .collect();
            Wkt::MultiPoint(MultiPoint::new(points, dim))
        }
        Wkt::MultiLineString(lines) => {
            let (lines, dim) = lines.into_inner();
            let lines = lines
                .into_iter()
                .filter_map(|line| snap_line_string(line, grid))
                .collect();
            Wkt::MultiLineString(MultiLineString::new(lines, dim))
        }
        Wkt::MultiPolygon(polygons) => {
            let (polygons, dim) = polygons.into_inner();
            let polygons = polygons
                .into_iter()
                .filter_map(|polygon| snap_polygon(polygon, grid))
                .collect();
            Wkt::MultiPolygon(MultiPolygon::new(polygons, dim))
        }
        Wkt::GeometryCollection(collection) => {
            let (members, dim) = collection.into_inner();
            // Members that end up empty, including those that were, are dropped.
            let members = members
                .into_iter()
                .map(|member| snap_geometry(member, grid))
                .filter(|member| !is_empty(member))
                .collect();
            Wkt::GeometryCollection(GeometryCollection::new(members, dim))
        }
    }
}

fn snap_point(point: Point<f64>, grid: &Grid) -> Point<f64> {
    let (coord, dim) = point.into_inner();
    Point::new(coord.map(|c| grid.snap(c)), dim)
}

/// The snapped linestring, or `None` if fewer than 2 points are left.
fn snap_line_string(line: LineString<f64>, grid: &Grid) -> Option<LineString<f64>> {
    let (coords, dim) = line.into_inner();
    let coords = grid.snap_coords(coords);
    (coords.len() >= 2).then(|| LineString::new(coords, dim))
}

/// The snapped polygon without the rings that collapsed (fewer than 4 points left), or `None` if
/// its exterior ring collapsed.
fn snap_polygon(polygon: Polygon<f64>, grid: &Grid) -> Option<Polygon<f64>> {
    let (rings, dim) = polygon.into_inner();
    let mut rings = rings.into_iter().map(|ring| {
        let (coords, ring_dim) = ring.into_inner();
        let coords = grid.snap_coords(coords);
        (coords.len() >= 4).then(|| LineString::new(coords, ring_dim))
    });
    let exterior = rings.next()??;
    let snapped = std::iter::once(exterior).chain(rings.flatten()).collect();
    Some(Polygon::new(snapped, dim))
}

fn is_empty(geom: &Wkt<f64>) -> bool {
    match geom {
        Wkt::Point(point) => point.coord().is_none(),
        Wkt::LineString(line) => line.coords().is_empty(),
        Wkt::Polygon(polygon) => polygon.rings().is_empty(),
        Wkt::MultiPoint(points) => points.points().is_empty(),
        Wkt::MultiLineString(lines) => lines.line_strings().is_empty(),
        Wkt::MultiPolygon(polygons) => polygons.polygons().is_empty(),
        Wkt::GeometryCollection(collection) => collection.geometries().is_empty(),
    }
}
