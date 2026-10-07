//! ST_MinimumClearance.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geos::Geom;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::to_geos;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_MinimumClearance(geometry g).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a geometry's minimum clearance.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the minimum clearance of a geometry: the shortest distance by which a vertex could move before the geometry became invalid (a robustness measure). Returns Infinity when the geometry has no minimum clearance (a single point, or an empty geometry).",
    syntax_example = "ST_MinimumClearance(g)",
    argument(name = "g", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MinimumClearance;

impl MinimumClearance {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MinimumClearance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MinimumClearance {
    fn name(&self) -> &str {
        "st_minimumclearance"
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
        Ok(minimum_clearance_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn minimum_clearance_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = MinimumClearanceKernel;
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct MinimumClearanceKernel;

impl GeometryKernel for MinimumClearanceKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(to_geos(geom)?.minimum_clearance()?))
    }
}
