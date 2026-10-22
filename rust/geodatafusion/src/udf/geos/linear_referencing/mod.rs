//! Linear referencing.

mod locate_between;

pub use locate_between::LocateBetween;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(LocateBetween.into());
}
