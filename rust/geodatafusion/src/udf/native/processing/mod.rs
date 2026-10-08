//! Geometry processing.

mod simplify;
mod simplify_vw;

pub use simplify::Simplify;
pub use simplify_vw::SimplifyVW;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Simplify.into());
    session_context.register_udf(SimplifyVW.into());
}
