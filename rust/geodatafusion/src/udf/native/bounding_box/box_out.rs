use std::sync::Arc;

use arrow_array::StringArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::r#box::boxes;
use crate::udf::native::bounding_box::util::box_text::{write_box2d, write_box3d};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Data Types"),
    description = "Returns the text of a box2d, BOX(xmin ymin,xmax ymax), as a cast of a box2d to text does. A geometry is cast to box2d first, so an empty one gives NULL.",
    syntax_example = "box2d_out(box)",
    argument(name = "box", description = "box2d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Box2DOut;

impl Box2DOut {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Box2DOut {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Box2DOut {
    fn name(&self) -> &str {
        "box2d_out"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(box_out_impl(args, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Data Types"),
    description = "Returns the text of a box3d, BOX3D(xmin ymin zmin,xmax ymax zmax), as a cast of a box3d to text does. A box2d or geometry is cast to box3d first: Z is 0 without Z, and an empty geometry gives NULL.",
    syntax_example = "box3d_out(box)",
    argument(name = "box", description = "box3d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Box3DOut;

impl Box3DOut {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Box3DOut {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Box3DOut {
    fn name(&self) -> &str {
        "box3d_out"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(box_out_impl(args, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn box_out_impl(args: ScalarFunctionArgs, include_z: bool) -> GeoDataFusionResult<ColumnarValue> {
    let write = if include_z { write_box3d } else { write_box2d };
    let texts: StringArray = boxes(&args, include_z)?
        .iter()
        .map(|rect| rect.as_ref().map(write))
        .collect();
    Ok(ColumnarValue::Array(Arc::new(texts)))
}
