use geodatafusion::udf::native::trajectory::{
    CPAWithin, ClosestPointOfApproach, DistanceCPA, IsValidTrajectory,
};

use crate::impl_udf;

impl_udf!(CPAWithin, PyCPAWithin, "CPAWithin");
impl_udf!(
    ClosestPointOfApproach,
    PyClosestPointOfApproach,
    "ClosestPointOfApproach"
);
impl_udf!(DistanceCPA, PyDistanceCPA, "DistanceCPA");
impl_udf!(IsValidTrajectory, PyIsValidTrajectory, "IsValidTrajectory");
