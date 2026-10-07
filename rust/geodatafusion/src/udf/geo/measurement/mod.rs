mod length;

pub use length::Length;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Length.into());
}
