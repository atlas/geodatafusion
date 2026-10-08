mod geometry;

pub use geometry::Geometry;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Geometry::new().into());
}
