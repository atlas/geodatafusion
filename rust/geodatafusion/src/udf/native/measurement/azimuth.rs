use std::f64::consts::PI;
use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, PointTrait};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodesic::{GeodeticArgument, GeodeticPair, geodetic_pair, xy};
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Azimuth(geometry geom1, geometry geom2) and ST_Azimuth(geography geog1,
/// geography geog2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the north-based azimuth of a line between two points.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the azimuth in radians of the line from geom1 to geom2: the angle clockwise from north (the positive Y axis) in [0, 2π). Both must be points; coincident points give NULL, and an empty point is an error. For geography the azimuth is the geodesic's at geom1, on the SRID's spheroid; geometry with geography is measured as geography.",
    syntax_example = "ST_Azimuth(geom1, geom2)",
    argument(name = "geom1", description = "geometry or geography point"),
    argument(name = "geom2", description = "geometry or geography point")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Azimuth;

impl Azimuth {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Azimuth {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Azimuth {
    fn name(&self) -> &str {
        "st_azimuth"
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
        Ok(azimuth_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn azimuth_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    if let Some(GeodeticPair { first, second }) = geodetic_pair(name, &args, None)? {
        let kernel = AzimuthKernel {
            geom2: OwnedColumn::try_new(&second.0, &second.1, args.number_rows)?,
            geodetic: Some(&first),
        };
        let result: Float64Array = map_geometry(first.geometries.as_ref(), &kernel)?;
        return Ok(ColumnarValue::Array(Arc::new(result)));
    }
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = AzimuthKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        geodetic: None,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AzimuthKernel<'a> {
    geom2: OwnedColumn,
    /// The geodesics to measure with for geography; planar without.
    geodetic: Option<&'a GeodeticArgument>,
}

impl GeometryKernel for AzimuthKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let (p, q) = (point(geom1, self.geodetic)?, point(geom2, self.geodetic)?);
        if p == q {
            return Ok(None);
        }
        Ok(Some(match self.geodetic {
            Some(geodetic) => geodetic.geodesic.azimuth(p, q),
            // North is π/2 counterclockwise from the X axis.
            None => (2.0 * PI + PI / 2.0 - (q.1 - p.1).atan2(q.0 - p.0)) % (2.0 * PI),
        }))
    }
}

/// The coordinates of a point, or PostGIS's error for anything else.
fn point(
    geom: &impl GeometryTrait<T = f64>,
    geodetic: Option<&GeodeticArgument>,
) -> GeoDataFusionResult<(f64, f64)> {
    let GeometryType::Point(point) = geom.as_type() else {
        return Err(match geodetic {
            Some(_) => exec_datafusion_err!(
                "st_azimuth: ST_Azimuth(geography, geography) is only valid for point inputs"
            ),
            None => exec_datafusion_err!("st_azimuth: Argument must be POINT geometries"),
        }
        .into());
    };
    let Some(coord) = point.coord() else {
        return Err(match geodetic {
            Some(_) => exec_datafusion_err!(
                "st_azimuth: ST_Azimuth(geography, geography) cannot work with empty points"
            ),
            None => exec_datafusion_err!("st_azimuth: Error extracting point"),
        }
        .into());
    };
    Ok(xy(&coord))
}
