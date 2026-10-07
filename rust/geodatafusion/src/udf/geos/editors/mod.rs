//! Geometry editors.

mod normalize;
mod snap;

pub use normalize::Normalize;
pub use snap::Snap;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Normalize.into());
    session_context.register_udf(Snap.into());
}
