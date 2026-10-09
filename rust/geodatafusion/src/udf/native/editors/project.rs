use std::f64::consts::PI;
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
use geo_traits::{GeometryTrait, GeometryType, PointTrait};
use wkt::types::{Coord, Point};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, dimension, owned_coord};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_Project(geometry geom1, float distance, float azimuth)
/// - ST_Project(geometry geom1, geometry geom2, float distance)
///
/// The geography forms wait for the geography type.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Geometry, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "distance", "azimuth"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a point projected from a start point by a distance and bearing (azimuth).
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the POINT at distance from geom1 in the direction of azimuth, in radians clockwise from north, in the units of the SRS. The second form continues the line from geom1 through geom2 by distance past geom2, measured in 2D, and extends the Z and M geom1 has too (a missing one in geom2 counts as 0). As in PostGIS, an empty point gives NULL and the SRIDs aren't compared. The geography forms aren't supported yet.",
    syntax_example = "ST_Project(geom1, distance, azimuth)",
    alternative_syntax = "ST_Project(geom1, geom2, distance)",
    argument(name = "geom1", description = "geometry, a POINT"),
    argument(
        name = "distance",
        description = "float8, or geom2: a POINT geometry giving the direction"
    ),
    argument(
        name = "azimuth",
        description = "float8 in radians, or distance in the second form"
    ),
    related_udf(name = "st_azimuth"),
    related_udf(name = "st_translate")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Project;

impl Project {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Project {
    fn name(&self) -> &str {
        "st_project"
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
        Ok(project_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn project_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    // Coercion makes a distance Float64 and a direction point a geometry.
    let direction = if args.arg_fields[1].data_type().is_numeric() {
        Direction::Azimuth {
            distance: optional_float_arg(&args, 1, 0.0)?,
            azimuth: optional_float_arg(&args, 2, 0.0)?,
        }
    } else {
        Direction::Towards {
            point: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
            distance: optional_float_arg(&args, 2, 0.0)?,
        }
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &direction, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

enum Direction {
    Azimuth {
        distance: Float64Array,
        azimuth: Float64Array,
    },
    Towards {
        point: OwnedColumn,
        distance: Float64Array,
    },
}

impl GeometryKernel for Direction {
    type Output = Point<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Point<f64>>> {
        let GeometryType::Point(point) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_project: Argument must be POINT geometry").into());
        };
        let dim = dimension(geom.dim());
        let start = point.coord().map(|c| owned_coord(&c));
        let projected = match self {
            Direction::Azimuth { distance, azimuth } => {
                if distance.is_null(row) || azimuth.is_null(row) {
                    return Ok(None);
                }
                let Some(start) = start else {
                    return Ok(None);
                };
                let theta = azimuth_angle(azimuth.value(row));
                let distance = distance.value(row);
                Coord {
                    x: start.x + distance * theta.cos(),
                    y: start.y + distance * theta.sin(),
                    ..start
                }
            }
            Direction::Towards { point, distance } => {
                let (Some(towards), false) = (point.get(row), distance.is_null(row)) else {
                    return Ok(None);
                };
                let wkt::Wkt::Point(towards) = towards else {
                    return Err(exec_datafusion_err!(
                        "st_project: Arguments must be POINT geometries"
                    )
                    .into());
                };
                let (Some(start), Some(end)) = (start, towards.coord()) else {
                    return Ok(None);
                };
                let (dx, dy) = (end.x - start.x, end.y - start.y);
                let length = dx.hypot(dy);
                // The same point twice gives no direction; PostGIS returns the second point.
                let factor = if length == 0.0 {
                    0.0
                } else {
                    distance.value(row) / length
                };
                let extend = |from: f64, to: f64| to + (to - from) * factor;
                Coord {
                    x: extend(start.x, end.x),
                    y: extend(start.y, end.y),
                    z: start.z.map(|z| extend(z, end.z.unwrap_or(0.0))),
                    m: start.m.map(|m| extend(m, end.m.unwrap_or(0.0))),
                }
            }
        };
        Ok(Some(Point::new(Some(projected), dim)))
    }
}

/// The mathematical angle (counter-clockwise from east) of an azimuth (clockwise from north),
/// computed as PostGIS's results show: the azimuth normalised to [0, 2π), then 2.5π minus it,
/// less 2π when that is above 2π. This keeps PostGIS's rounding, such as sin(2π) for an azimuth
/// of π/2.
fn azimuth_angle(azimuth: f64) -> f64 {
    let azimuth = azimuth.rem_euclid(2.0 * PI);
    let theta = 2.5 * PI - azimuth;
    if theta > 2.0 * PI {
        theta - 2.0 * PI
    } else {
        theta
    }
}
