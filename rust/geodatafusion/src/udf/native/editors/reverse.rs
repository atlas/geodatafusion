use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_line_strings;
use crate::util::signature::single_geometry;

/// Return the geometry with vertex order reversed.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry with the order of the vertices of every linestring and polygon ring reversed. Points, and the order of the members of collections, are unchanged.",
    syntax_example = "ST_Reverse(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_forcepolygoncw")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Reverse;

impl Reverse {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Reverse {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Reverse {
    fn name(&self) -> &str {
        "st_reverse"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
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

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(reverse_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn reverse_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &ReverseKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ReverseKernel;

impl GeometryKernel for ReverseKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(map_line_strings(geom, &|_, mut coords| {
            coords.reverse();
            coords
        })))
    }
}
