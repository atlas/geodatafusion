//! ST_ShortestLine and ST_ClosestPoint: the nearest points of two geometries.

use std::sync::LazyLock;

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
use wkt::types::{Coord, Dimension, LineString, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::distance::{Measure, Xy, nearest};
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ShortestLine(geometry geom1, geometry geom2), and the same for ST_ClosestPoint.
/// The geography forms wait for the geography type.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the 2D shortest line between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D line between the nearest points of two geometries, from geom1 to geom2; its length is ST_Distance. If several pairs are equally near, the first found is returned, which can differ from PostGIS's choice. Returns NULL if either geometry is empty. The result is 2D. The geography form isn't supported yet.",
    syntax_example = "ST_ShortestLine(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_closestpoint"),
    related_udf(name = "st_longestline"),
    related_udf(name = "st_distance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ShortestLine;

impl ShortestLine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShortestLine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ShortestLine {
    fn name(&self) -> &str {
        "st_shortestline"
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
        Ok(nearest_impl(self.name(), args, Output::Line)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 2D point on geom1 that is closest to geom2.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D point of geom1 nearest to geom2: the first point of ST_ShortestLine. If several points are equally near, the first found is returned, which can differ from PostGIS's choice. Returns NULL if either geometry is empty. The result is 2D. The geography form isn't supported yet.",
    syntax_example = "ST_ClosestPoint(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_shortestline"),
    related_udf(name = "st_distance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ClosestPoint;

impl ClosestPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClosestPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ClosestPoint {
    fn name(&self) -> &str {
        "st_closestpoint"
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
        Ok(nearest_impl(self.name(), args, Output::Point)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// What to return of the nearest points.
#[derive(Debug, Clone, Copy)]
enum Output {
    /// The first point.
    Point,
    /// The line between them.
    Line,
}

fn nearest_impl(
    name: &str,
    args: ScalarFunctionArgs,
    output: Output,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = NearestKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        output,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct NearestKernel {
    geom2: OwnedColumn,
    output: Output,
}

impl GeometryKernel for NearestKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        Ok(nearest(geom1, geom2).map(|measure| measure_geometry(measure, self.output)))
    }
}

fn measure_geometry(Measure { points, .. }: Measure, output: Output) -> Wkt<f64> {
    let coord = |(x, y): Xy| Coord {
        x,
        y,
        z: None,
        m: None,
    };
    match output {
        Output::Point => Wkt::Point(Point::new(Some(coord(points[0])), Dimension::XY)),
        Output::Line => Wkt::LineString(LineString::new(
            points.into_iter().map(coord).collect(),
            Dimension::XY,
        )),
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_shortest_line_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(ShortestLine.into());
        ctx.register_udf(ClosestPoint.into());
        ctx.register_udf(GeomFromText::default().into());

        for function in ["ST_ShortestLine", "ST_ClosestPoint"] {
            let sql = format!(
                "SELECT {function}(ST_GeomFromText('POINT(0 0)', 3857), \
                 ST_GeomFromText('LINESTRING(1 1, 2 0)', 3857))"
            );
            assert_wkb_output(&ctx, &sql, 3857).await;
        }
    }
}
