//! Measurement functions.

mod area;

pub use area::Area;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Area.into());
}
