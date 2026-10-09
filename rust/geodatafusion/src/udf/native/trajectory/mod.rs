mod is_valid_trajectory;

pub use is_valid_trajectory::IsValidTrajectory;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(IsValidTrajectory.into());
}
