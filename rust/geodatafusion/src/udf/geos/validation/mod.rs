//! Geometry validation.

mod is_valid_detail;
mod is_valid_reason;
mod make_valid;

pub use is_valid_detail::IsValidDetail;
pub use is_valid_reason::IsValidReason;
pub use make_valid::MakeValid;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(IsValidReason.into());
    crate::util::set_returning::register_record(session_context, IsValidDetail.into());
    session_context.register_udf(MakeValid.into());
}
