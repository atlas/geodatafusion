//! Spatial relationships.

mod dwithin;
mod dwithin3d;
pub(crate) mod relate_match;

pub use dwithin::DWithin;
pub use dwithin3d::{DFullyWithin3D, DWithin3D, Intersects3D};
pub use relate_match::RelateMatch;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(DFullyWithin3D.into());
    session_context.register_udf(DWithin.into());
    session_context.register_udf(DWithin3D.into());
    session_context.register_udf(Intersects3D.into());
    session_context.register_udf(RelateMatch.into());
}
