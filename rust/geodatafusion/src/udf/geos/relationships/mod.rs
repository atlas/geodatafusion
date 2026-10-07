//! Spatial relationships.

mod predicates;
mod relate;

pub use predicates::{
    Contains, ContainsProperly, CoveredBy, Covers, Crosses, Disjoint, Equals, Intersects, Overlaps,
    Touches, Within,
};
pub use relate::Relate;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Contains.into());
    session_context.register_udf(ContainsProperly.into());
    session_context.register_udf(CoveredBy.into());
    session_context.register_udf(Covers.into());
    session_context.register_udf(Crosses.into());
    session_context.register_udf(Disjoint.into());
    session_context.register_udf(Equals.into());
    session_context.register_udf(Intersects.into());
    session_context.register_udf(Overlaps.into());
    session_context.register_udf(Relate.into());
    session_context.register_udf(Touches.into());
    session_context.register_udf(Within.into());
}
