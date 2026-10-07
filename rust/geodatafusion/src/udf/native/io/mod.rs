//! Geometry Input and Output

mod as_binary;
mod as_text;
mod geo_hash;
mod geom_from_geo_hash;
mod geom_from_text;
mod util;
mod wkb;

pub use as_binary::{AsBinary, AsEWKB, AsHEXEWKB};
pub use as_text::{AsEWKT, AsText};
pub use geo_hash::GeoHash;
pub use geom_from_geo_hash::{Box2DFromGeoHash, GeomFromGeoHash, PointFromGeoHash};
pub use geom_from_text::{GeomFromEWKT, GeomFromText};
pub use wkb::GeomFromWKB;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsBinary.into());
    session_context.register_udf(AsEWKB.into());
    session_context.register_udf(AsHEXEWKB.into());
    session_context.register_udf(GeomFromWKB::default().into());
    session_context.register_udf(AsText.into());
    session_context.register_udf(AsEWKT.into());
    session_context.register_udf(GeoHash.into());
    session_context.register_udf(PointFromGeoHash.into());
    session_context.register_udf(GeomFromGeoHash.into());
    session_context.register_udf(Box2DFromGeoHash.into());
    session_context.register_udf(GeomFromText::default().into());
    session_context.register_udf(GeomFromEWKT.into());
}
