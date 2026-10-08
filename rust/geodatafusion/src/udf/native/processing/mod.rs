//! Geometry processing.

mod simplify;

pub use simplify::Simplify;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Simplify.into());
}
