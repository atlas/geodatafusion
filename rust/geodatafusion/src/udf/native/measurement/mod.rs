//! Measurement functions.

mod area;
pub(crate) mod distance;

pub use area::Area;
pub use distance::Distance;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Area.into());
    session_context.register_udf(Distance.into());
}
