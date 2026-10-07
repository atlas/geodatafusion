use std::sync::{Arc, LazyLock};

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, PolygonTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the number of interior rings (holes) of a POLYGON. Returns NULL for any other geometry type, including a MULTIPOLYGON.",
    syntax_example = "ST_NumInteriorRings(a_polygon)",
    argument(name = "a_polygon", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NumInteriorRings;

impl NumInteriorRings {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for NumInteriorRings {
    fn default() -> Self {
        Self::new()
    }
}

static ALIASES: LazyLock<Vec<String>> = LazyLock::new(|| vec!["st_numinteriorring".to_string()]);

impl ScalarUDFImpl for NumInteriorRings {
    fn name(&self) -> &str {
        "st_numinteriorrings"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn aliases(&self) -> &[String] {
        &ALIASES
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(num_interior_rings_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn num_interior_rings_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int32Array = map_geometry(geometries.as_ref(), &NumInteriorRingsKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// The number of interior rings of a polygon; NULL for every other type, including
/// multipolygons, as in PostGIS.
struct NumInteriorRingsKernel;

impl GeometryKernel for NumInteriorRingsKernel {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        let count = match geom.as_type() {
            GeometryType::Polygon(polygon) => polygon.num_interiors(),
            GeometryType::Rect(_) | GeometryType::Triangle(_) => 0,
            _ => return Ok(None),
        };
        let count = i32::try_from(count)
            .map_err(|_| exec_datafusion_err!("too many rings for an integer: {count}"))?;
        Ok(Some(count))
    }
}
