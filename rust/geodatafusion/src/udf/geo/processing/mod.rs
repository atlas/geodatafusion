mod simplify;

pub use simplify::{Simplify, SimplifyPreserveTopology, SimplifyVW};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Simplify.into());
    session_context.register_udf(SimplifyPreserveTopology.into());
    session_context.register_udf(SimplifyVW.into());
}
