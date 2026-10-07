//! Spatial relationships.

mod dwithin;
pub(crate) mod relate_match;

pub use dwithin::DWithin;
pub use relate_match::RelateMatch;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(DWithin.into());
    session_context.register_udf(RelateMatch.into());
}
