use std::sync::Arc;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::from_arrow_array;
use geoarrow_schema::{CoordType, Dimension, Metadata, PointType};

use crate::error::GeoDataFusionResult;
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a POINT which is guaranteed to lie in the interior of a surface.",
    syntax_example = "ST_PointOnSurface(geometry)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointOnSurface {
    coord_type: CoordType,
}

impl PointOnSurface {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for PointOnSurface {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl ScalarUDFImpl for PointOnSurface {
    fn name(&self) -> &str {
        "st_pointonsurface"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(return_field_impl(args, self.coord_type)?)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(interior_point_impl(args, self.coord_type)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn return_field_impl(
    args: ReturnFieldArgs,
    coord_type: CoordType,
) -> GeoDataFusionResult<FieldRef> {
    let metadata = Arc::new(Metadata::try_from(args.arg_fields[0].as_ref()).unwrap_or_default());
    let output_type = PointType::new(Dimension::XY, metadata).with_coord_type(coord_type);
    Ok(Arc::new(output_type.to_field("", true)))
}

fn interior_point_impl(
    args: ScalarFunctionArgs,
    coord_type: CoordType,
) -> GeoDataFusionResult<ColumnarValue> {
    let arrays = ColumnarValue::values_to_arrays(&args.args)?;
    let geo_array = from_arrow_array(&arrays[0], &args.arg_fields[0])?;
    let result = geoarrow_expr_geo::interior_point(&geo_array, coord_type)?;
    Ok(ColumnarValue::Array(result.into_array_ref()))
}
