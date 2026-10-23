use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::distance::distance;
use crate::udf::native::util::geodesic::{GeodeticArgument, GeodeticPair, geodetic_pair};
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Distance(geometry g1, geometry g2).
/// PostGIS: ST_Distance(geometry g1, geometry g2) and ST_Distance(geography geog1, geography
/// geog2, boolean use_spheroid = true).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2", "use_spheroid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the distance between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "For geometry types returns the minimum 2D Cartesian (planar) distance between two geometries, in projected units (spatial ref units). A point inside a polygon is at distance 0 from it. Returns NULL if either geometry is empty. Z and M are ignored. geometry_distance_centroid, the function behind the <-> operator, is the same function. For geography the distance is in metres, along the geodesic on the SRID's spheroid, or the great circle on a sphere of the spheroid's mean radius if use_spheroid is false, rounded to 8 decimals as in PostGIS; geometry with geography, or given use_spheroid, is measured as geography. Only points, multipoints and their collections are supported as geography yet.",
    syntax_example = "ST_Distance(g1, g2)",
    alternative_syntax = "g1 <-> g2",
    alternative_syntax = "ST_Distance(geog1, geog2, use_spheroid)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    argument(name = "use_spheroid", description = "boolean, default true")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance {
    aliases: Vec<String>,
}

impl Distance {
    pub fn new() -> Self {
        Self {
            aliases: vec!["geometry_distance_centroid".to_string()],
        }
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

    fn aliases(&self) -> &[String] {
        &self.aliases
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
    if let Some(GeodeticPair { first, second }) = geodetic_pair(name, &args, Some(2))? {
        let kernel = GeodeticDistanceKernel {
            name,
            geom2: OwnedColumn::try_new(&second.0, &second.1, args.number_rows)?,
            first: &first,
        };
        let result: Float64Array = map_geometry(first.geometries.as_ref(), &kernel)?;
        return Ok(ColumnarValue::Array(Arc::new(result)));
    }
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

struct GeodeticDistanceKernel<'a> {
    name: &'a str,
    first: &'a GeodeticArgument,
    geom2: OwnedColumn,
}

impl GeometryKernel for GeodeticDistanceKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let (Some(geom2), Some(spheroid)) = (self.geom2.get(row), self.first.use_spheroid(row))
        else {
            return Ok(None);
        };
        let distance = self
            .first
            .geodesic
            .points_distance(self.name, geom1, geom2, spheroid)?;
        // PostGIS rounds geography distances to 8 decimals.
        Ok(distance.map(|distance| (distance * 1e8).round() / 1e8))
    }
}
