mod simplify;

pub use simplify::{Simplify, SimplifyVW};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Simplify.into());
    session_context.register_udf(SimplifyVW.into());
}
