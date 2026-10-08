//! Coverages: sets of polygons that don't overlap and share edges exactly.

mod coverage_union;

pub use coverage_union::CoverageUnion;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udaf(CoverageUnion.into());
}
