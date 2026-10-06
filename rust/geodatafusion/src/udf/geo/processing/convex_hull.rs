use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geoarrow_schema::CoordType;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_result, wkb_return_field};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes the convex hull of a geometry. The convex hull is the smallest convex geometry that encloses all geometries in the input.",
    syntax_example = "ST_ConvexHull(geometry)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ConvexHull;

impl ConvexHull {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConvexHull {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ConvexHull {
    fn name(&self) -> &str {
        "st_convexhull"
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
        Ok(convex_hull_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn convex_hull_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    // geoarrow-expr-geo builds a native array; the result is converted to WKB.
    let result = geoarrow_expr_geo::convex_hull(&geometries, CoordType::default())?;
    wkb_result(&result, &args.return_field)
}
