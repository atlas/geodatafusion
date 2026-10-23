//! ST_Area.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiPolygonTrait, PolygonTrait, RectTrait, TriangleTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodesic::{Geodesic, GeodeticArgument, geodetic_argument};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Area(geometry g1) and ST_Area(geography geog, boolean use_spheroid = true).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Boolean]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Returns the area of a geometry.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the area of a polygonal geometry: the area of each polygon's shell minus the areas of its holes, summed over the parts of a collection. Points and lines have zero area, as does an empty geometry. Z and M are ignored. For geography the area is in square metres, on the SRID's spheroid, and a polygon whose holes are larger than its shell is an error, as in PostGIS. Geometry given use_spheroid is measured as geography. The sphere (use_spheroid false) isn't supported yet.",
    syntax_example = "ST_Area(geom)",
    alternative_syntax = "ST_Area(geog, use_spheroid)",
    argument(name = "geom", description = "geometry"),
    argument(name = "geog", description = "geography"),
    argument(name = "use_spheroid", description = "boolean, default true")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Area;

impl Area {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Area {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Area {
    fn name(&self) -> &str {
        "st_area"
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
        Ok(area_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn area_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    if let Some(geodetic) = geodetic_argument(name, &args, 0, 1)? {
        let kernel = GeodeticAreaKernel(&geodetic);
        let result: Float64Array = map_geometry(geodetic.geometries.as_ref(), &kernel)?;
        return Ok(ColumnarValue::Array(Arc::new(result)));
    }
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &AreaKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AreaKernel;

impl GeometryKernel for AreaKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(area(geom)))
    }
}

struct GeodeticAreaKernel<'a>(&'a GeodeticArgument);

impl GeometryKernel for GeodeticAreaKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        match self.0.use_spheroid(row) {
            None => Ok(None),
            Some(true) => geodetic_area(geom, &self.0.geodesic).map(Some),
            Some(false) => Err(exec_datafusion_err!(
                "st_area: the area of geography on the sphere (use_spheroid false) isn't supported yet"
            )
            .into()),
        }
    }
}

/// The area of geography on the spheroid, summed as [`area`] sums it.
fn geodetic_area(
    geom: &impl GeometryTrait<T = f64>,
    geodesic: &Geodesic,
) -> GeoDataFusionResult<f64> {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_geodetic_area(polygon, geodesic),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_geodetic_area(&polygon, geodesic))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| geodetic_area(&member, geodesic))
            .sum(),
        GeometryType::Point(_)
        | GeometryType::LineString(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::MultiLineString(_)
        | GeometryType::Line(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_) => Ok(0.0),
    }
}

fn polygon_geodetic_area(
    polygon: &impl PolygonTrait<T = f64>,
    geodesic: &Geodesic,
) -> GeoDataFusionResult<f64> {
    geodesic.area(polygon).ok_or_else(|| {
        exec_datafusion_err!("st_area: the area is negative: a hole is larger than its shell")
            .into()
    })
}

pub(crate) fn area(geom: &impl GeometryTrait<T = f64>) -> f64 {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_area(polygon),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_area(&polygon))
            .sum(),
        GeometryType::GeometryCollection(collection) => {
            collection.geometries().map(|member| area(&member)).sum()
        }
        GeometryType::Rect(rect) => {
            (rect.max().x() - rect.min().x()) * (rect.max().y() - rect.min().y())
        }
        GeometryType::Triangle(triangle) => {
            let [a, b, c] = triangle.coords().map(|coord| (coord.x(), coord.y()));
            ring_area([a, b, c, a].into_iter())
        }
        GeometryType::Point(_)
        | GeometryType::LineString(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::MultiLineString(_)
        | GeometryType::Line(_) => 0.0,
    }
}

/// The shell's area minus the holes' areas. PostGIS doesn't clamp it, so a hole larger than
/// its shell gives a negative area.
fn polygon_area(polygon: &impl PolygonTrait<T = f64>) -> f64 {
    let Some(shell) = polygon.exterior() else {
        return 0.0;
    };
    polygon
        .interiors()
        .fold(ring_area(xy(&shell)), |area, hole| {
            area - ring_area(xy(&hole))
        })
}

fn xy(ring: &impl LineStringTrait<T = f64>) -> impl Iterator<Item = (f64, f64)> {
    ring.coords().map(|coord| (coord.x(), coord.y()))
}

/// The unsigned area of a closed ring.
///
/// This is the shoelace formula with the coordinates shifted by the first X, as JTS computes
/// it (`Area.ofRingSigned`). The shift keeps precision for coordinates far from the origin,
/// and this summation order agrees with PostGIS to the last digit, where geo's doesn't.
fn ring_area(mut ring: impl Iterator<Item = (f64, f64)>) -> f64 {
    let (Some(first), Some(mut current)) = (ring.next(), ring.next()) else {
        return 0.0;
    };
    let x0 = first.0;
    let mut previous_y = first.1;
    let mut sum = 0.0;
    for next in ring {
        sum += (current.0 - x0) * (previous_y - next.1);
        previous_y = current.1;
        current = next;
    }
    (sum / 2.0).abs()
}
