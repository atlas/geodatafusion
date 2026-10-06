mod topological;

pub use topological::{
    Contains, CoveredBy, Covers, Crosses, Disjoint, Equals, Intersects, Overlaps, Touches, Within,
};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Contains.into());
    session_context.register_udf(CoveredBy.into());
    session_context.register_udf(Covers.into());
    session_context.register_udf(Crosses.into());
    session_context.register_udf(Disjoint.into());
    session_context.register_udf(Equals.into());
    session_context.register_udf(Intersects.into());
    session_context.register_udf(Overlaps.into());
    session_context.register_udf(Touches.into());
    session_context.register_udf(Within.into());
}
