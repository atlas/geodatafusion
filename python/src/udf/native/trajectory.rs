use geodatafusion::udf::native::trajectory::IsValidTrajectory;

use crate::impl_udf;

impl_udf!(IsValidTrajectory, PyIsValidTrajectory, "IsValidTrajectory");
