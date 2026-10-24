//! ST_3DDistance, ST_3DMaxDistance, ST_3DClosestPoint, ST_3DShortestLine and ST_3DLongestLine:
//! distances and nearest and farthest points in 3D.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
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
use crate::udf::native::util::distance3d::{
    Measure3D, Xyz, distance_3d, farthest_points_3d, max_distance_3d, nearest_points_3d,
};
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_3DDistance(geometry g1, geometry g2), and the same for the others.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the 3D minimum distance between two geometries in projected units.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the minimum 3D Cartesian distance between two geometries, in projected units. A polygon is taken to be planar: a point above or below it is measured to its plane, through the average of its shell's vertices. If either geometry has no Z, it is the 2D distance, as in PostGIS, which takes the missing Z to be any value. Returns NULL if either geometry is empty.",
    syntax_example = "ST_3DDistance(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_distance"),
    related_udf(name = "st_3dshortestline"),
    related_udf(name = "st_3dmaxdistance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance3D;

impl Distance3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Distance3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Distance3D {
    fn name(&self) -> &str {
        "st_3ddistance"
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
        Ok(distance_impl(self.name(), args, Extreme::Nearest)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 3D maximum distance between two geometries in projected units.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the largest 3D Cartesian distance between two geometries, in projected units: the distance between their farthest vertices, of the polygons' shells. If either geometry has no Z, it is the 2D distance, as ST_MaxDistance measures it. Returns NULL if either geometry is empty.",
    syntax_example = "ST_3DMaxDistance(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_maxdistance"),
    related_udf(name = "st_3dlongestline"),
    related_udf(name = "st_3ddistance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MaxDistance3D;

impl MaxDistance3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MaxDistance3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MaxDistance3D {
    fn name(&self) -> &str {
        "st_3dmaxdistance"
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
        Ok(distance_impl(self.name(), args, Extreme::Farthest)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 3D point on g1 that is closest to g2.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 3D point of g1 nearest to g2, the first point of ST_3DShortestLine. If only one geometry has Z, it is the 2D nearest point, at the Z of the nearest point of the geometry with Z. If neither has, the result is 2D. Returns NULL if either geometry is empty.",
    syntax_example = "ST_3DClosestPoint(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_3dshortestline"),
    related_udf(name = "st_closestpoint"),
    related_udf(name = "st_3ddistance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ClosestPoint3D;

impl ClosestPoint3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClosestPoint3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ClosestPoint3D {
    fn name(&self) -> &str {
        "st_3dclosestpoint"
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
        Ok(points_impl(
            self.name(),
            args,
            Extreme::Nearest,
            Output::Point,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 3D shortest line between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 3D line between the nearest points of two geometries, from g1 to g2; its length is ST_3DDistance. If several pairs are equally near, the first found is returned, which can differ from PostGIS's choice. If only one geometry has Z, it is the 2D shortest line, both ends at the Z of its end on the geometry with Z. If neither has, the result is 2D. Returns NULL if either geometry is empty.",
    syntax_example = "ST_3DShortestLine(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_3dclosestpoint"),
    related_udf(name = "st_shortestline"),
    related_udf(name = "st_3dlongestline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ShortestLine3D;

impl ShortestLine3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShortestLine3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ShortestLine3D {
    fn name(&self) -> &str {
        "st_3dshortestline"
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
        Ok(points_impl(
            self.name(),
            args,
            Extreme::Nearest,
            Output::Line,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 3D longest line between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 3D line between the farthest vertices of two geometries, of the polygons' shells, from g1 to g2. If several pairs are equally far, the first found is returned. If only one geometry has Z, the other's vertices are taken at the lowest and the highest Z of the one with Z, as in PostGIS, so the line can be longer than ST_3DMaxDistance, which is 2D then. If neither has, the result is 2D. Returns NULL if either geometry is empty.",
    syntax_example = "ST_3DLongestLine(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    related_udf(name = "st_3dmaxdistance"),
    related_udf(name = "st_longestline"),
    related_udf(name = "st_3dshortestline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LongestLine3D;

impl LongestLine3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LongestLine3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LongestLine3D {
    fn name(&self) -> &str {
        "st_3dlongestline"
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
        Ok(points_impl(
            self.name(),
            args,
            Extreme::Farthest,
            Output::Line,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Which distance to measure.
#[derive(Debug, Clone, Copy)]
enum Extreme {
    Nearest,
    Farthest,
}

/// What to return of the points measured between.
#[derive(Debug, Clone, Copy)]
enum Output {
    /// The point on the first geometry.
    Point,
    /// The line between them.
    Line,
}

fn distance_impl(
    name: &str,
    args: ScalarFunctionArgs,
    extreme: Extreme,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DistanceKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        extreme,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceKernel {
    geom2: OwnedColumn,
    extreme: Extreme,
}

impl GeometryKernel for DistanceKernel {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        Ok(match self.extreme {
            Extreme::Nearest => distance_3d(geom1, geom2),
            Extreme::Farthest => max_distance_3d(geom1, geom2),
        })
    }
}

fn points_impl(
    name: &str,
    args: ScalarFunctionArgs,
    extreme: Extreme,
    output: Output,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = PointsKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        extreme,
        output,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PointsKernel {
    geom2: OwnedColumn,
    extreme: Extreme,
    output: Output,
}

impl GeometryKernel for PointsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let measured = match self.extreme {
            Extreme::Nearest => nearest_points_3d(geom1, geom2),
            Extreme::Farthest => farthest_points_3d(geom1, geom2),
        };
        Ok(measured.map(|(measure, has_z)| measure_geometry(measure, has_z, self.output)))
    }
}

fn measure_geometry(Measure3D { points, .. }: Measure3D, has_z: bool, output: Output) -> Wkt<f64> {
    let (dimension, coord) = if has_z {
        (
            Dimension::XYZ,
            (|[x, y, z]: Xyz| Coord {
                x,
                y,
                z: Some(z),
                m: None,
            }) as fn(Xyz) -> _,
        )
    } else {
        (
            Dimension::XY,
            (|[x, y, _]: Xyz| Coord {
                x,
                y,
                z: None,
                m: None,
            }) as fn(Xyz) -> _,
        )
    };
    match output {
        Output::Point => Wkt::Point(Point::new(Some(coord(points[0])), dimension)),
        Output::Line => Wkt::LineString(LineString::new(
            points.into_iter().map(coord).collect(),
            dimension,
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
    async fn test_3d_lines_return_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(ClosestPoint3D.into());
        ctx.register_udf(ShortestLine3D.into());
        ctx.register_udf(LongestLine3D.into());
        ctx.register_udf(GeomFromText::default().into());

        for function in ["ST_3DClosestPoint", "ST_3DShortestLine", "ST_3DLongestLine"] {
            let sql = format!(
                "SELECT {function}(ST_GeomFromText('POINT(0 0 0)', 3857), \
                 ST_GeomFromText('LINESTRING(1 1 1, 2 0 2)', 3857))"
            );
            assert_wkb_output(&ctx, &sql, 3857).await;
        }
    }
}
