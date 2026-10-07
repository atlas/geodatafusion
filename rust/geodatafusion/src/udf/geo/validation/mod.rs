mod is_valid;

pub use is_valid::IsValid;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(IsValid.into());
}
