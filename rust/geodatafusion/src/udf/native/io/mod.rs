//! Geometry Input and Output

mod as_text;
mod geom_from_text;
mod util;
mod wkb;

pub use as_text::{AsEWKT, AsText};
pub use geom_from_text::{GeomFromEWKT, GeomFromText};
pub use wkb::{AsBinary, GeomFromWKB};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsBinary.into());
    session_context.register_udf(GeomFromWKB::default().into());
    session_context.register_udf(AsText.into());
    session_context.register_udf(AsEWKT.into());
    session_context.register_udf(GeomFromText::default().into());
    session_context.register_udf(GeomFromEWKT.into());
}
