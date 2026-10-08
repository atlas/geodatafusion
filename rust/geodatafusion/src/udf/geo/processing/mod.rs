mod simplify;

pub use simplify::SimplifyVW;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(SimplifyVW.into());
}
