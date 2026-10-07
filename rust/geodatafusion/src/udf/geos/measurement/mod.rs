//! Measurement functions.

mod minimum_clearance;
mod minimum_clearance_line;

pub use minimum_clearance::MinimumClearance;
pub use minimum_clearance_line::MinimumClearanceLine;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(MinimumClearance.into());
    session_context.register_udf(MinimumClearanceLine.into());
}
