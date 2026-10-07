//! Geometry validation.

mod make_valid;

pub use make_valid::MakeValid;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(MakeValid.into());
}
