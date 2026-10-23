use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryCollectionTrait, GeometryTrait, GeometryType, MultiPolygonTrait};

use crate::error::GeoDataFusionResult;
use crate::udf::native::measurement::length3d::polygon_perimeter;
use crate::udf::native::util::geodesic::{Geodesic, GeodeticArgument, geodetic_argument};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Perimeter(geometry g1) and ST_Perimeter(geography geog, boolean
/// use_spheroid = true).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Boolean]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

static ALIASES: LazyLock<Vec<String>> = LazyLock::new(|| vec!["st_perimeter2d".to_string()]);

/// Returns the length of the boundary of a polygonal geometry or geography.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D length of the polygon rings of a geometry, interior rings and the polygons of a GEOMETRYCOLLECTION included. Lines and points have perimeter 0; Z and M are ignored. ST_Perimeter2D is an alias. For geography the perimeter is in metres, along geodesics on the SRID's spheroid, or along great circles on a sphere of the spheroid's mean radius if use_spheroid is false, with each segment's rise in Z added as PostGIS does. Geometry given use_spheroid is measured as geography.",
    syntax_example = "ST_Perimeter(g1)",
    alternative_syntax = "ST_Perimeter(geog, use_spheroid)",
    argument(name = "g1", description = "geometry"),
    argument(name = "geog", description = "geography"),
    argument(name = "use_spheroid", description = "boolean, default true"),
    related_udf(name = "st_length"),
    related_udf(name = "st_3dperimeter")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Perimeter;

impl Perimeter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Perimeter {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Perimeter {
    fn name(&self) -> &str {
        "st_perimeter"
    }

    fn aliases(&self) -> &[String] {
        &ALIASES
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(perimeter_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn perimeter_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    if let Some(geodetic) = geodetic_argument(name, &args, 0, 1)? {
        let kernel = GeodeticPerimeterKernel(&geodetic);
        let result: Float64Array = map_geometry(geodetic.geometries.as_ref(), &kernel)?;
        return Ok(ColumnarValue::Array(Arc::new(result)));
    }
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &PerimeterKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct PerimeterKernel;

impl GeometryKernel for PerimeterKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(perimeter(geom)))
    }
}

struct GeodeticPerimeterKernel<'a>(&'a GeodeticArgument);

impl GeometryKernel for GeodeticPerimeterKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self
            .0
            .use_spheroid(row)
            .map(|spheroid| geodetic_perimeter(geom, &self.0.geodesic, spheroid)))
    }
}

/// The perimeter of geography: its polygon rings' lengths, as [`perimeter`] sums them.
fn geodetic_perimeter(
    geom: &impl GeometryTrait<T = f64>,
    geodesic: &Geodesic,
    spheroid: bool,
) -> f64 {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => geodesic.perimeter(polygon, spheroid),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| geodesic.perimeter(&polygon, spheroid))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| geodetic_perimeter(&member, geodesic, spheroid))
            .sum(),
        _ => 0.0,
    }
}

fn perimeter(geom: &impl GeometryTrait<T = f64>) -> f64 {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_perimeter(polygon, false),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_perimeter(&polygon, false))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| perimeter(&member))
            .sum(),
        // As the polygon PostGIS would see.
        GeometryType::Rect(_) | GeometryType::Triangle(_) => perimeter(&to_owned_geometry(geom)),
        _ => 0.0,
    }
}
