//! Geometry editors.

mod normalize;

pub use normalize::Normalize;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Normalize.into());
}
