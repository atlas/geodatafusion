//! ST_DistanceSphere, ST_DistanceSpheroid and ST_LengthSpheroid: geometry on longitude/latitude,
//! measured on a sphere or a given spheroid.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, BooleanArray, Float64Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, GeometryType, MultiLineStringTrait, MultiPolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodesic::{
    Geodesic, GeodeticArgument, GeodeticPair, geography_pair, parse_spheroid,
};
use crate::util::args::{optional_float_arg, optional_text_arg};
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS's default for ST_DistanceSpheroid: WGS 84.
const WGS84: &str = r#"SPHEROID["WGS 84",6378137,298.257223563]"#;

/// PostGIS: ST_DistanceSphere(geometry geom1, geometry geom2) and ST_DistanceSphere(geometry
/// geom1, geometry geom2, double precision radius).
static SPHERE_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Float],
];

static SPHERE_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2", "radius"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns minimum distance in meters between two lon/lat geometries using a spherical earth model.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the distance in metres between two longitude/latitude geometries on a sphere: without radius, the sphere of the SRID's spheroid's mean radius, as ST_Distance(geography, geography, false) measures it (rounded to 8 decimals, the SRID 4326 by default and longitude/latitude); with radius, the angle between the nearest points times radius.",
    syntax_example = "ST_DistanceSphere(geom1, geom2, radius)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "radius", description = "double precision")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DistanceSphere;

impl DistanceSphere {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DistanceSphere {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DistanceSphere {
    fn name(&self) -> &str {
        "st_distancesphere"
    }

    fn signature(&self) -> &Signature {
        &SPHERE_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, SPHERE_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_sphere_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_sphere_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let use_spheroid = BooleanArray::from(vec![false; args.number_rows]);
    let GeodeticPair { first, second } = geography_pair(name, &args, use_spheroid)?;
    let kernel = DistanceSphereKernel {
        first: &first,
        geom2: OwnedColumn::try_new(&second.0, &second.1, args.number_rows)?,
        radius: (args.args.len() > 2)
            .then(|| optional_float_arg(&args, 2, 0.0))
            .transpose()?,
    };
    let result: Float64Array = map_geometry(first.geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceSphereKernel<'a> {
    first: &'a GeodeticArgument,
    geom2: OwnedColumn,
    /// The radius argument, if given.
    radius: Option<Float64Array>,
}

impl GeometryKernel for DistanceSphereKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let geodesic = &self.first.geodesic;
        match &self.radius {
            Some(radius) if radius.is_null(row) => Ok(None),
            Some(radius) => Ok(geodesic
                .geometry_angle(geom1, geom2)
                .map(|angle| angle * radius.value(row))),
            // As ST_Distance(geography, geography, false), rounded to 8 decimals.
            None => Ok(geodesic
                .geometry_distance(geom1, geom2, false)
                .map(|distance| (distance * 1e8).round() / 1e8)),
        }
    }
}

/// PostGIS: ST_DistanceSpheroid(geometry geom1, geometry geom2, spheroid
/// measurement_spheroid = WGS 84). geodatafusion takes the spheroid as text.
static SPHEROID_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Text],
];

static SPHEROID_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2", "measurement_spheroid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the minimum distance between two lon/lat geometries using a spheroidal earth model.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the distance in metres between two longitude/latitude geometries along the geodesic on a spheroid, given as text: SPHEROID[\"name\",semi-major axis,inverse flattening], WGS 84 by default. The nearest points are found on the sphere, as in PostGIS. The SRID isn't checked, an empty geometry gives -1, and collections are an error, as in PostGIS.",
    syntax_example = "ST_DistanceSpheroid(geom1, geom2, measurement_spheroid)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(
        name = "measurement_spheroid",
        description = "text, default 'SPHEROID[\"WGS 84\",6378137,298.257223563]'"
    )
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DistanceSpheroid;

impl DistanceSpheroid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DistanceSpheroid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DistanceSpheroid {
    fn name(&self) -> &str {
        "st_distancespheroid"
    }

    fn signature(&self) -> &Signature {
        &SPHEROID_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, SPHEROID_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_spheroid_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_spheroid_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DistanceSpheroidKernel {
        name,
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        spheroid: optional_text_arg(&args, 2, WGS84)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceSpheroidKernel<'a> {
    name: &'a str,
    geom2: OwnedColumn,
    spheroid: StringArray,
}

impl GeometryKernel for DistanceSpheroidKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let Some(geodesic) = spheroid_geodesic(self.name, &self.spheroid, row)? else {
            return Ok(None);
        };
        if is_collection(geom1) || is_collection(geom2) {
            return Err(
                exec_datafusion_err!("{}: Only point/line/polygon supported.", self.name).into(),
            );
        }
        // PostGIS gives -1 for an empty geometry here.
        let distance = geodesic.geometry_distance(geom1, geom2, true);
        Ok(Some(distance.unwrap_or(-1.0)))
    }
}

fn is_collection(geom: &impl GeometryTrait<T = f64>) -> bool {
    matches!(geom.as_type(), GeometryType::GeometryCollection(_))
}

/// PostGIS: ST_LengthSpheroid(geometry a_geometry, spheroid a_spheroid). geodatafusion takes
/// the spheroid as text.
static LENGTH_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Text]];

static LENGTH_SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Returns the 2D or 3D length/perimeter of a lon/lat geometry on a spheroid.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the length in metres of a longitude/latitude geometry along geodesics on a spheroid, given as text: SPHEROID[\"name\",semi-major axis,inverse flattening]. Lines and polygon rings both count, in collections too, and each segment's rise in Z is added by Pythagoras, as in PostGIS. The SRID isn't checked. Unlike PostGIS, which gives 0, a spheroid of practically no flattening (an inverse flattening of 1e30 or more) measures as a sphere.",
    syntax_example = "ST_LengthSpheroid(a_geometry, a_spheroid)",
    argument(name = "a_geometry", description = "geometry"),
    argument(name = "a_spheroid", description = "text")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LengthSpheroid;

impl LengthSpheroid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LengthSpheroid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LengthSpheroid {
    fn name(&self) -> &str {
        "st_lengthspheroid"
    }

    fn signature(&self) -> &Signature {
        &LENGTH_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, LENGTH_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(length_spheroid_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn length_spheroid_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LengthSpheroidKernel {
        name,
        spheroid: optional_text_arg(&args, 1, WGS84)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct LengthSpheroidKernel<'a> {
    name: &'a str,
    spheroid: StringArray,
}

impl GeometryKernel for LengthSpheroidKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(geodesic) = spheroid_geodesic(self.name, &self.spheroid, row)? else {
            return Ok(None);
        };
        Ok(Some(length_spheroid(geom, &geodesic)))
    }
}

/// The length of lines and polygon rings alike.
fn length_spheroid(geom: &impl GeometryTrait<T = f64>, geodesic: &Geodesic) -> f64 {
    match geom.as_type() {
        GeometryType::LineString(line) => geodesic.length(line, true),
        GeometryType::MultiLineString(lines) => lines
            .line_strings()
            .map(|line| geodesic.length(&line, true))
            .sum(),
        GeometryType::Polygon(polygon) => geodesic.perimeter(polygon, true),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| geodesic.perimeter(&polygon, true))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| length_spheroid(&member, geodesic))
            .sum(),
        GeometryType::Point(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::Line(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_) => 0.0,
    }
}

/// The geodesics of row `row`'s spheroid text; `None` for NULL.
fn spheroid_geodesic(
    name: &str,
    spheroids: &StringArray,
    row: usize,
) -> GeoDataFusionResult<Option<Geodesic>> {
    if spheroids.is_null(row) {
        return Ok(None);
    }
    let (a, rf) =
        parse_spheroid(spheroids.value(row)).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
    Ok(Some(Geodesic::with_flattening(a, 1.0 / rf)))
}
