use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodesic::{Geodesic, GeodeticArgument, geodetic_argument};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D Cartesian length of the geometry if it is a LineString or MultiLineString, or the sum of the lengths of the lines in a GeometryCollection. For areal geometries 0 is returned; use ST_Perimeter instead. Points and empty geometries have length 0. Z and M are ignored. For geography the length is in metres, along geodesics on the SRID's spheroid, or along great circles on a sphere of the spheroid's mean radius if use_spheroid is false, with each segment's rise in Z added as PostGIS does. Geometry given use_spheroid is measured as geography.",
    syntax_example = "ST_Length(geom)",
    alternative_syntax = "ST_Length(geog, use_spheroid)",
    argument(name = "geom", description = "geometry"),
    argument(name = "geog", description = "geography"),
    argument(name = "use_spheroid", description = "boolean, default true")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Length;

impl Length {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Length {
    fn default() -> Self {
        Self::new()
    }
}

/// PostGIS: ST_Length(geometry a_2dlinestring) and ST_Length(geography geog, boolean
/// use_spheroid = true).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Boolean]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

static ALIASES: LazyLock<Vec<String>> = LazyLock::new(|| vec!["st_length2d".to_string()]);

impl ScalarUDFImpl for Length {
    fn name(&self) -> &str {
        "st_length"
    }

    fn aliases(&self) -> &[String] {
        ALIASES.as_slice()
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
        Ok(length_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn length_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    if let Some(geodetic) = geodetic_argument(name, &args, 0, 1)? {
        let kernel = GeodeticLengthKernel(&geodetic);
        let result: Float64Array = map_geometry(geodetic.geometries.as_ref(), &kernel)?;
        return Ok(ColumnarValue::Array(Arc::new(result)));
    }
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &LengthKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct LengthKernel;

impl GeometryKernel for LengthKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(length(geom)))
    }
}

/// The length of the lines of a geometry, collections included.
struct GeodeticLengthKernel<'a>(&'a GeodeticArgument);

impl GeometryKernel for GeodeticLengthKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self
            .0
            .use_spheroid(row)
            .map(|spheroid| geodetic_length(geom, &self.0.geodesic, spheroid)))
    }
}

/// The length of geography: its lines' lengths, as [`length`] sums them.
fn geodetic_length(geom: &impl GeometryTrait<T = f64>, geodesic: &Geodesic, spheroid: bool) -> f64 {
    match geom.as_type() {
        GeometryType::LineString(line) => geodesic.length(line, spheroid),
        GeometryType::MultiLineString(lines) => lines
            .line_strings()
            .map(|line| geodesic.length(&line, spheroid))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| geodetic_length(&member, geodesic, spheroid))
            .sum(),
        GeometryType::Point(_)
        | GeometryType::Line(_)
        | GeometryType::Polygon(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::MultiPolygon(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_) => 0.0,
    }
}

fn length(geom: &impl GeometryTrait<T = f64>) -> f64 {
    match geom.as_type() {
        GeometryType::LineString(line) => line_length(line),
        GeometryType::MultiLineString(lines) => {
            lines.line_strings().map(|line| line_length(&line)).sum()
        }
        GeometryType::GeometryCollection(collection) => {
            collection.geometries().map(|member| length(&member)).sum()
        }
        GeometryType::Line(line) => segment_length(&line.start(), &line.end()),
        GeometryType::Point(_)
        | GeometryType::Polygon(_)
        | GeometryType::MultiPoint(_)
        | GeometryType::MultiPolygon(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_) => 0.0,
    }
}

fn line_length(line: &impl LineStringTrait<T = f64>) -> f64 {
    let mut coords = line.coords();
    let Some(mut previous) = coords.next() else {
        return 0.0;
    };
    let mut length = 0.0;
    for coord in coords {
        length += segment_length(&previous, &coord);
        previous = coord;
    }
    length
}

fn segment_length(a: &impl CoordTrait<T = f64>, b: &impl CoordTrait<T = f64>) -> f64 {
    let (dx, dy) = (b.x() - a.x(), b.y() - a.y());
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use arrow_array::types::Float64Type;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test_linestring_length() {
        let ctx = SessionContext::new();

        ctx.register_udf(Length::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0, 3 4)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Float64Type>().value(0);
        assert_eq!(val, 5.0);
    }

    #[tokio::test]
    async fn test_point_length() {
        let ctx = SessionContext::new();

        ctx.register_udf(Length::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_Length(ST_GeomFromText('POINT(1 2)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Float64Type>().value(0);
        assert_eq!(val, 0.0);
    }

    #[tokio::test]
    async fn test_multilinestring_length() {
        let ctx = SessionContext::new();

        ctx.register_udf(Length::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_Length(ST_GeomFromText('MULTILINESTRING((0 0, 3 4), (0 0, 4 3))'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Float64Type>().value(0);
        assert_eq!(val, 10.0); // 5.0 + 5.0
    }

    #[tokio::test]
    async fn test_polygon_length() {
        let ctx = SessionContext::new();

        ctx.register_udf(Length::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_Length(ST_GeomFromText('POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Float64Type>().value(0);
        assert_eq!(val, 0.0); // Polygons return 0 for length
    }
}
