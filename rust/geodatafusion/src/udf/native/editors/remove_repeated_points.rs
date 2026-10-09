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
use wkt::Wkt;
use wkt::types::{
    Coord, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
    Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_RemoveRepeatedPoints(geometry geom, float8 tolerance = 0.0).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "tolerance"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a version of a geometry with duplicate points removed.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry without repeated points: in lines and rings, points equal to the last one kept (in every ordinate), or within tolerance of it in 2D; in a MULTIPOINT, points equal to or within tolerance of any point kept, and empty points. The last point of a line or ring is always kept, replacing the last kept point if it is within tolerance of it. Lines keep at least 2 points and rings 4. A negative tolerance counts as 0.",
    syntax_example = "ST_RemoveRepeatedPoints(geom, tolerance)",
    argument(name = "geom", description = "geometry"),
    argument(name = "tolerance", description = "float8, default 0"),
    related_udf(name = "st_simplify"),
    related_udf(name = "st_snaptogrid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RemoveRepeatedPoints;

impl RemoveRepeatedPoints {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RemoveRepeatedPoints {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RemoveRepeatedPoints {
    fn name(&self) -> &str {
        "st_removerepeatedpoints"
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
        Ok(remove_repeated_points_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn remove_repeated_points_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = RemoveRepeatedPointsKernel {
        tolerance: optional_float_arg(&args, 1, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct RemoveRepeatedPointsKernel {
    tolerance: Float64Array,
}

impl GeometryKernel for RemoveRepeatedPointsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.tolerance.is_null(row) {
            return Ok(None);
        }
        let tolerance = self.tolerance.value(row).max(0.0);
        Ok(Some(remove_repeated(to_owned_geometry(geom), tolerance)))
    }
}

fn remove_repeated(geom: Wkt<f64>, tolerance: f64) -> Wkt<f64> {
    let line = |line: LineString<f64>, min_points| {
        let (coords, dim) = line.into_inner();
        LineString::new(thin(coords, tolerance, min_points), dim)
    };
    let polygon = |polygon: Polygon<f64>| {
        let (rings, dim) = polygon.into_inner();
        Polygon::new(rings.into_iter().map(|ring| line(ring, 4)).collect(), dim)
    };
    match geom {
        Wkt::Point(_) => geom,
        Wkt::LineString(l) => Wkt::LineString(line(l, 2)),
        Wkt::Polygon(p) => Wkt::Polygon(polygon(p)),
        Wkt::MultiPoint(points) => {
            let (points, dim) = points.into_inner();
            let mut kept: Vec<Point<f64>> = vec![];
            for point in points {
                let Some(coord) = point.coord() else {
                    continue;
                };
                // Points are compared in 2D, even with no tolerance.
                let repeated = kept.iter().any(|other| {
                    other.coord().is_some_and(|other| {
                        (coord.x - other.x).hypot(coord.y - other.y) <= tolerance
                    })
                });
                if !repeated {
                    kept.push(point);
                }
            }
            Wkt::MultiPoint(MultiPoint::new(kept, dim))
        }
        Wkt::MultiLineString(lines) => {
            let (lines, dim) = lines.into_inner();
            Wkt::MultiLineString(MultiLineString::new(
                lines.into_iter().map(|l| line(l, 2)).collect(),
                dim,
            ))
        }
        Wkt::MultiPolygon(polygons) => {
            let (polygons, dim) = polygons.into_inner();
            Wkt::MultiPolygon(MultiPolygon::new(
                polygons.into_iter().map(polygon).collect(),
                dim,
            ))
        }
        Wkt::GeometryCollection(collection) => {
            let (members, dim) = collection.into_inner();
            Wkt::GeometryCollection(GeometryCollection::new(
                members
                    .into_iter()
                    .map(|member| remove_repeated(member, tolerance))
                    .collect(),
                dim,
            ))
        }
    }
}

/// The points of a line or ring without repeats, as PostGIS removes them: a point equal to the
/// last one kept (with no tolerance) or within tolerance of it (in 2D) is dropped, except the
/// last point, which replaces the last one kept instead (unless that is the first). No more
/// points are dropped than leave `min_points`.
fn thin(coords: Vec<Coord<f64>>, tolerance: f64, min_points: usize) -> Vec<Coord<f64>> {
    let count = coords.len();
    if count <= min_points {
        return coords;
    }
    let repeated = |a: &Coord<f64>, b: &Coord<f64>| {
        if tolerance == 0.0 {
            a == b
        } else {
            (a.x - b.x).hypot(a.y - b.y) <= tolerance
        }
    };
    let mut budget = count - min_points;
    let mut kept: Vec<Coord<f64>> = Vec::with_capacity(count);
    for (index, coord) in coords.into_iter().enumerate() {
        let Some(last) = kept.last() else {
            kept.push(coord);
            continue;
        };
        if budget > 0 && repeated(&coord, last) {
            if index + 1 < count {
                budget -= 1;
                continue;
            }
            if kept.len() > 1 {
                kept.pop();
            }
        }
        kept.push(coord);
    }
    kept
}
