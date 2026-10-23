mod geometry;

pub use geometry::{Geography, Geometry};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Geometry::new().into());
    session_context.register_udf(Geography::new().into());
}
