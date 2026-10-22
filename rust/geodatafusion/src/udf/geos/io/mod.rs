//! Geometry output.

mod as_mvt_geom;

pub use as_mvt_geom::AsMVTGeom;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsMVTGeom.into());
}
