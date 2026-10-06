//! Geometry Input and Output

mod as_text;
mod util;
mod wkb;
mod wkt;

pub use as_text::AsText;
pub use wkb::{AsBinary, GeomFromWKB};
pub use wkt::GeomFromText;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsBinary.into());
    session_context.register_udf(GeomFromWKB::default().into());
    session_context.register_udf(AsText.into());
    session_context.register_udf(GeomFromText::default().into());
}
