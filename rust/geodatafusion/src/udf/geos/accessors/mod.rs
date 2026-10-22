//! Geometry accessors.

mod is_ring;
mod is_simple;

pub use is_ring::IsRing;
pub use is_simple::IsSimple;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(IsRing.into());
    session_context.register_udf(IsSimple.into());
}
