use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{Dimensions, GeometryTrait};
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::{optional_bool_arg, optional_float_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_FilterByM(geometry geom, double precision min, double precision max = null,
/// boolean returnM = false).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "min", "max", "returnM"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Removes vertices based on their M value.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns the geometry with only the vertices whose M is between min and max, inclusive (max NULL, the default, for no upper bound). M is dropped unless returnM is true. Lines left with fewer than 2 points and rings with fewer than 4 are dropped (a polygon with its exterior), as are emptied collection members; rings aren't closed again. A geometry without M is returned unchanged, and min above max is an error. As in PostGIS, a NULL min is the smallest positive double, so M values of 0 and below go, and a NULL returnM is false.",
    syntax_example = "ST_FilterByM(geom, min, max, returnM)",
    argument(name = "geom", description = "geometry"),
    argument(name = "min", description = "float8"),
    argument(name = "max", description = "float8, default NULL (no upper bound)"),
    argument(name = "returnM", description = "boolean, default false"),
    related_udf(name = "st_locatebetween")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct FilterByM;

impl FilterByM {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FilterByM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for FilterByM {
    fn name(&self) -> &str {
        "st_filterbym"
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
        Ok(filter_by_m_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn filter_by_m_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let max = match args.args.get(2) {
        Some(_) => optional_float_arg(&args, 2, 0.0)?,
        None => Float64Array::new_null(args.number_rows),
    };
    let kernel = FilterByMKernel {
        min: optional_float_arg(&args, 1, 0.0)?,
        max,
        return_m: optional_bool_arg(&args, 3, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct FilterByMKernel {
    min: Float64Array,
    /// NULL for no upper bound.
    max: Float64Array,
    return_m: BooleanArray,
}

impl GeometryKernel for FilterByMKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // The function isn't STRICT. PostGIS reads a NULL min as the smallest positive double
        // (C's DBL_MIN), which drops M values of 0 and below, and a NULL returnM as false.
        let min = if self.min.is_null(row) {
            f64::MIN_POSITIVE
        } else {
            self.min.value(row)
        };
        let max = (!self.max.is_null(row)).then(|| self.max.value(row));
        if max.is_some_and(|max| min > max) {
            return Err(exec_datafusion_err!(
                "st_filterbym: Min-value cannot be larger than Max value"
            )
            .into());
        }
        let geom = to_owned_geometry(geom);
        if !matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
            return Ok(Some(geom));
        }
        let filter = Filter {
            min,
            max: max.unwrap_or(f64::INFINITY),
            return_m: !self.return_m.is_null(row) && self.return_m.value(row),
        };
        Ok(Some(filter.geometry(geom)))
    }
}

struct Filter {
    min: f64,
    max: f64,
    return_m: bool,
}

impl Filter {
    fn dim(&self, dim: Dimension) -> Dimension {
        match (dim, self.return_m) {
            (Dimension::XYZM, false) => Dimension::XYZ,
            (Dimension::XYM, false) => Dimension::XY,
            (dim, _) => dim,
        }
    }

    fn keep(&self, coord: &Coord<f64>) -> Option<Coord<f64>> {
        let m = coord.m?;
        (m >= self.min && m <= self.max).then_some(Coord {
            m: if self.return_m { coord.m } else { None },
            ..*coord
        })
    }

    /// The kept coordinates of a line or ring, or `None` if fewer than `min_points` are left.
    fn coords(&self, line: LineString<f64>, min_points: usize) -> Option<LineString<f64>> {
        let (coords, dim) = line.into_inner();
        let coords: Vec<_> = coords.iter().filter_map(|c| self.keep(c)).collect();
        (coords.len() >= min_points).then(|| LineString::new(coords, self.dim(dim)))
    }

    fn polygon(&self, polygon: Polygon<f64>) -> Option<Polygon<f64>> {
        let (rings, dim) = polygon.into_inner();
        let mut rings = rings.into_iter();
        let exterior = self.coords(rings.next()?, 4)?;
        let rings = std::iter::once(exterior)
            .chain(rings.filter_map(|ring| self.coords(ring, 4)))
            .collect();
        Some(Polygon::new(rings, self.dim(dim)))
    }

    fn geometry(&self, geom: Wkt<f64>) -> Wkt<f64> {
        match geom {
            Wkt::Point(point) => {
                let (coord, dim) = point.into_inner();
                Wkt::Point(Point::new(coord.and_then(|c| self.keep(&c)), self.dim(dim)))
            }
            Wkt::LineString(line) => {
                let dim = self.dim(line.dimension());
                Wkt::LineString(self.coords(line, 2).unwrap_or(LineString::new(vec![], dim)))
            }
            Wkt::Polygon(polygon) => {
                let dim = self.dim(polygon.dimension());
                Wkt::Polygon(self.polygon(polygon).unwrap_or(Polygon::new(vec![], dim)))
            }
            Wkt::MultiPoint(points) => {
                let (points, dim) = points.into_inner();
                let dim = self.dim(dim);
                let points = points
                    .into_iter()
                    .filter_map(|point| point.coord().and_then(|c| self.keep(c)))
                    .map(|coord| Point::new(Some(coord), dim))
                    .collect();
                Wkt::MultiPoint(MultiPoint::new(points, dim))
            }
            Wkt::MultiLineString(lines) => {
                let (lines, dim) = lines.into_inner();
                Wkt::MultiLineString(MultiLineString::new(
                    lines
                        .into_iter()
                        .filter_map(|l| self.coords(l, 2))
                        .collect(),
                    self.dim(dim),
                ))
            }
            Wkt::MultiPolygon(polygons) => {
                let (polygons, dim) = polygons.into_inner();
                Wkt::MultiPolygon(MultiPolygon::new(
                    polygons
                        .into_iter()
                        .filter_map(|p| self.polygon(p))
                        .collect(),
                    self.dim(dim),
                ))
            }
            Wkt::GeometryCollection(collection) => {
                let (members, dim) = collection.into_inner();
                Wkt::GeometryCollection(GeometryCollection::new(
                    members
                        .into_iter()
                        .map(|member| self.geometry(member))
                        .filter(|member| !is_geometry_topologically_empty(member))
                        .collect(),
                    self.dim(dim),
                ))
            }
        }
    }
}
