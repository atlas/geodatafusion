use std::sync::Arc;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::RectBuilder;
use geoarrow_schema::{BoxType, Dimension, GeoArrowType};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::{BoundingRect, BoundsKernel};
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::map_geometry;
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Data Types"),
    description = "Returns a box2d representing the 2D extent of the geometry, or NULL for an empty geometry.",
    syntax_example = "Box2D(geometry)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Box2D;

impl Box2D {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Box2D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Box2D {
    fn name(&self) -> &str {
        "box2d"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(return_field_impl(self.name(), args, Dimension::XY))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(box_impl(args, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Data Types"),
    description = "Returns a box3d representing the 3D extent of the geometry, or NULL for an empty geometry. The Z of a geometry without Z is 0.",
    syntax_example = "Box3D(geometry)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Box3D;

impl Box3D {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Box3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Box3D {
    fn name(&self) -> &str {
        "box3d"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(return_field_impl(self.name(), args, Dimension::XYZ))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(box_impl(args, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn return_field_impl(name: &str, args: ReturnFieldArgs, dim: Dimension) -> FieldRef {
    let output_type = BoxType::new(dim, input_metadata(&args.arg_fields[0]));
    Arc::new(output_type.to_field(name, true))
}

fn box_impl(args: ScalarFunctionArgs, include_z: bool) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let rects: Vec<Option<BoundingRect>> =
        map_geometry(geometries.as_ref(), &BoundsKernel { include_z })?;

    let GeoArrowType::Rect(output_type) = GeoArrowType::from_arrow_field(&args.return_field)?
    else {
        return Err(internal_datafusion_err!("unexpected return field").into());
    };
    let mut builder = RectBuilder::with_capacity(output_type, rects.len());
    for rect in &rects {
        builder.push_rect(rect.as_ref());
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}

#[cfg(test)]
mod test {
    use approx::relative_eq;
    use datafusion::prelude::*;
    use geo_traits::{CoordTrait, RectTrait};
    use geoarrow_array::GeoArrowArrayAccessor;
    use geoarrow_array::array::RectArray;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test_2d() {
        let ctx = SessionContext::new();

        ctx.register_udf(Box2D::new().into());
        ctx.register_udf(GeomFromText::default().into());

        let out = ctx
            .sql("SELECT Box2D(ST_GeomFromText('LINESTRING(1 2, 3 4, 5 6)'));")
            .await
            .unwrap();
        let batch = out.collect().await.unwrap().into_iter().next().unwrap();
        let schema = batch.schema();
        let rect_array =
            RectArray::try_from((batch.columns()[0].as_ref(), schema.field(0))).unwrap();
        let rect = rect_array.value(0).unwrap();

        assert!(relative_eq!(rect.min().x(), 1.0));
        assert!(relative_eq!(rect.min().y(), 2.0));
        assert!(relative_eq!(rect.max().x(), 5.0));
        assert!(relative_eq!(rect.max().y(), 6.0));
    }

    #[tokio::test]
    async fn test_3d() {
        let ctx = SessionContext::new();

        ctx.register_udf(Box3D::new().into());
        ctx.register_udf(GeomFromText::default().into());

        let out = ctx
            .sql("SELECT Box3D(ST_GeomFromText('LINESTRING Z(1 2 3, 3 4 5, 5 6 7)'));")
            .await
            .unwrap();
        let batch = out.collect().await.unwrap().into_iter().next().unwrap();
        let schema = batch.schema();
        let rect_array =
            RectArray::try_from((batch.columns()[0].as_ref(), schema.field(0))).unwrap();
        let rect = rect_array.value(0).unwrap();

        assert!(relative_eq!(rect.min().x(), 1.0));
        assert!(relative_eq!(rect.min().y(), 2.0));
        assert!(relative_eq!(rect.min().nth_or_panic(2), 3.0));
        assert!(relative_eq!(rect.max().x(), 5.0));
        assert!(relative_eq!(rect.max().y(), 6.0));
        assert!(relative_eq!(rect.max().nth_or_panic(2), 7.0));
    }
}
