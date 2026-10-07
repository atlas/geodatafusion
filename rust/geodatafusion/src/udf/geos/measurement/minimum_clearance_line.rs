//! ST_MinimumClearanceLine.

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
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_MinimumClearanceLine(geometry g).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the line of a geometry's minimum clearance.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the two-point linestring spanning a geometry's minimum clearance: the shortest distance by which a vertex could move before the geometry became invalid. Returns an empty linestring when the geometry has no minimum clearance (a single point, or an empty geometry). The result is 2D.",
    syntax_example = "ST_MinimumClearanceLine(g)",
    argument(name = "g", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MinimumClearanceLine;

impl MinimumClearanceLine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MinimumClearanceLine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MinimumClearanceLine {
    fn name(&self) -> &str {
        "st_minimumclearanceline"
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
        Ok(minimum_clearance_line_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn minimum_clearance_line_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = MinimumClearanceLineKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct MinimumClearanceLineKernel;

impl GeometryKernel for MinimumClearanceLineKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(from_geos(&to_geos(geom)?.minimum_clearance_line()?)?))
    }
}
