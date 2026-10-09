use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{Dimensions, GeometryTrait, GeometryType, LineStringTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::m;
use crate::util::signature::single_geometry;

/// Tests if the geometry is a valid trajectory.
#[user_doc(
    doc_section(label = "Trajectory Functions"),
    description = "Returns true if the geometry is a LINESTRING with M values that strictly increase from each vertex to the next. An empty line is valid; other types and lines without M are not.",
    syntax_example = "ST_IsValidTrajectory(line)",
    argument(name = "line", description = "geometry"),
    related_udf(name = "st_closestpointofapproach")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValidTrajectory;

impl IsValidTrajectory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsValidTrajectory {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValidTrajectory {
    fn name(&self) -> &str {
        "st_isvalidtrajectory"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_valid_trajectory_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_valid_trajectory_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsValidTrajectoryKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsValidTrajectoryKernel;

impl GeometryKernel for IsValidTrajectoryKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        let GeometryType::LineString(line) = geom.as_type() else {
            return Ok(Some(false));
        };
        if !matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
            return Ok(Some(false));
        }
        let measures: Vec<f64> = line.coords().map(|c| m(&c).unwrap_or(f64::NAN)).collect();
        Ok(Some(measures.windows(2).all(|pair| pair[1] > pair[0])))
    }
}
