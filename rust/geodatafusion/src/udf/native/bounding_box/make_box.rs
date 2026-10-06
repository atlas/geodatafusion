use std::sync::{Arc, LazyLock};

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::RectBuilder;
use geoarrow_schema::{BoxType, Dimension, GeoArrowType};
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::z;
use crate::util::signature::{Arg, coerce_args};

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Creates a box2d defined by two Point geometries. This is useful for doing range queries.",
    syntax_example = "ST_MakeBox2D(ST_Point(-989502.1875, 528439.5625), ST_Point(-987121.375, 529933.1875))",
    argument(name = "pointLowLeft", description = "geometry"),
    argument(name = "pointUpRight", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeBox2D;

impl MakeBox2D {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MakeBox2D {
    fn default() -> Self {
        Self::new()
    }
}

/// PostGIS: ST_MakeBox2D(geometry pointLowLeft, geometry pointUpRight).
static SIGNATURE_2D: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["pointLowLeft", "pointUpRight"])
        .expect("parameter names are valid for a user-defined signature")
});

impl ScalarUDFImpl for MakeBox2D {
    fn name(&self) -> &str {
        "st_makebox2d"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE_2D
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let output_type = BoxType::new(Dimension::XY, input_metadata(&args.arg_fields[0]));
        Ok(Arc::new(output_type.to_field(self.name(), true)))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(make_box_impl(self.name(), args, Dimension::XY)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Creates a box3d defined by two 3D Point geometries.",
    syntax_example = "ST_3DMakeBox(ST_MakePoint(-989502.1875, 528439.5625, 10),
	ST_MakePoint(-987121.375 ,529933.1875, 10))",
    argument(name = "pointLowLeft", description = "geometry"),
    argument(name = "pointUpRight", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeBox3D;

impl MakeBox3D {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MakeBox3D {
    fn default() -> Self {
        Self::new()
    }
}

/// PostGIS: ST_3DMakeBox(geometry point3DLowLeftBottom, geometry point3DUpRightTop).
static SIGNATURE_3D: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["point3DLowLeftBottom", "point3DUpRightTop"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Both box constructors take two points.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

impl ScalarUDFImpl for MakeBox3D {
    fn name(&self) -> &str {
        "st_3dmakebox"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE_3D
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let output_type = BoxType::new(Dimension::XYZ, input_metadata(&args.arg_fields[0]));
        Ok(Arc::new(output_type.to_field(self.name(), true)))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(make_box_impl(self.name(), args, Dimension::XYZ)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_box_impl(
    name: &str,
    args: ScalarFunctionArgs,
    dim: Dimension,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let kernel = PointCoordKernel { name };
    let lower: Vec<Option<[f64; 3]>> = map_geometry(geometry_array(&args, 0)?.as_ref(), &kernel)?;
    let upper: Vec<Option<[f64; 3]>> = map_geometry(geometry_array(&args, 1)?.as_ref(), &kernel)?;

    let GeoArrowType::Rect(output_type) = GeoArrowType::from_arrow_field(&args.return_field)?
    else {
        return Err(internal_datafusion_err!("{name}: unexpected return field").into());
    };
    let mut builder = RectBuilder::with_capacity(output_type, lower.len());
    for (lower, upper) in lower.iter().zip(&upper) {
        let (Some(lower), Some(upper)) = (lower, upper) else {
            // SQL NULL in, SQL NULL out.
            builder.push_null();
            continue;
        };
        let coord = |c: &[f64; 3]| Coord {
            x: c[0],
            y: c[1],
            z: (dim == Dimension::XYZ).then_some(c[2]),
            m: None,
        };
        if dim == Dimension::XY {
            // PostGIS orders the corners of a box2d, so either point may be the lower one. It
            // doesn't for ST_3DMakeBox.
            let min = [lower[0].min(upper[0]), lower[1].min(upper[1]), 0.0];
            let max = [lower[0].max(upper[0]), lower[1].max(upper[1]), 0.0];
            builder.push_min_max(&coord(&min), &coord(&max));
        } else {
            builder.push_min_max(&coord(lower), &coord(upper));
        }
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}

/// The X, Y and Z of a point (Z is 0 for 2D points, as in PostGIS).
struct PointCoordKernel<'a> {
    name: &'a str,
}

impl GeometryKernel for PointCoordKernel<'_> {
    type Output = [f64; 3];

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<[f64; 3]>> {
        let GeometryType::Point(point) = geom.as_type() else {
            return Err(exec_datafusion_err!("{}: arguments must be points", self.name).into());
        };
        // An EMPTY point has no coordinate, or NaN coordinates in WKB.
        let Some(coord) = point
            .coord()
            .filter(|c| !(c.x().is_nan() && c.y().is_nan()))
        else {
            return Err(exec_datafusion_err!("{}: args can not be empty points", self.name).into());
        };
        Ok(Some([coord.x(), coord.y(), z(&coord).unwrap_or(0.0)]))
    }
}

#[cfg(test)]
mod test {
    use approx::relative_eq;
    use datafusion::prelude::*;
    use geo_traits::{CoordTrait, RectTrait};
    use geoarrow_array::GeoArrowArrayAccessor;
    use geoarrow_array::array::RectArray;

    use super::*;
    use crate::udf::native::constructors::{Point, PointZ};

    #[tokio::test]
    async fn test_2d() {
        let ctx = SessionContext::new();

        ctx.register_udf(MakeBox2D::new().into());
        ctx.register_udf(Point.into());

        let out = ctx
            .sql("SELECT ST_MakeBox2D(ST_Point(0, 5), ST_Point(10, 20));")
            .await
            .unwrap();
        let batch = out.collect().await.unwrap().into_iter().next().unwrap();
        let schema = batch.schema();
        let rect_array =
            RectArray::try_from((batch.columns()[0].as_ref(), schema.field(0))).unwrap();
        let rect = rect_array.value(0).unwrap();

        assert!(relative_eq!(rect.min().x(), 0.0));
        assert!(relative_eq!(rect.min().y(), 5.0));
        assert!(relative_eq!(rect.max().x(), 10.0));
        assert!(relative_eq!(rect.max().y(), 20.0));
    }

    #[tokio::test]
    async fn test_3d() {
        let ctx = SessionContext::new();

        ctx.register_udf(MakeBox3D::new().into());
        ctx.register_udf(PointZ.into());

        let out = ctx
            .sql("SELECT ST_3DMakeBox(ST_PointZ(0, 5, 1), ST_PointZ(10, 20, 30));")
            .await
            .unwrap();
        let batch = out.collect().await.unwrap().into_iter().next().unwrap();
        let schema = batch.schema();
        let rect_array =
            RectArray::try_from((batch.columns()[0].as_ref(), schema.field(0))).unwrap();
        let rect = rect_array.value(0).unwrap();

        assert!(relative_eq!(rect.min().x(), 0.0));
        assert!(relative_eq!(rect.min().y(), 5.0));
        assert!(relative_eq!(rect.min().nth_or_panic(2), 1.0));
        assert!(relative_eq!(rect.max().x(), 10.0));
        assert!(relative_eq!(rect.max().y(), 20.0));
        assert!(relative_eq!(rect.max().nth_or_panic(2), 30.0));
    }
}
