//! Geometry processing.

mod chaikin_smoothing;
mod filter_by_m;
mod simplify;
mod simplify_vw;

pub use chaikin_smoothing::ChaikinSmoothing;
pub use filter_by_m::FilterByM;
pub use simplify::Simplify;
pub use simplify_vw::SimplifyVW;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Simplify.into());
    session_context.register_udf(SimplifyVW.into());
    session_context.register_udf(ChaikinSmoothing.into());
    session_context.register_udf(FilterByM.into());
}
