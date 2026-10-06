mod set_srid;
mod srid;

pub use set_srid::SetSRID;
pub use srid::SRID;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(SetSRID.into());
    session_context.register_udf(SRID.into());
}
