//! Trajectory functions.

mod closest_point_of_approach;
mod is_valid_trajectory;

pub use closest_point_of_approach::{CPAWithin, ClosestPointOfApproach, DistanceCPA};
pub use is_valid_trajectory::IsValidTrajectory;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(CPAWithin.into());
    session_context.register_udf(ClosestPointOfApproach.into());
    session_context.register_udf(DistanceCPA.into());
    session_context.register_udf(IsValidTrajectory.into());
}
