//! Geometry Input and Output

mod as_binary;
mod as_text;
mod geo_hash;
mod geom_from_geo_hash;
mod geom_from_text;
mod geom_from_wkb;
mod util;

pub use as_binary::{AsBinary, AsEWKB, AsHEXEWKB};
pub use as_text::{AsEWKT, AsText};
pub use geo_hash::GeoHash;
pub use geom_from_geo_hash::{Box2DFromGeoHash, GeomFromGeoHash, PointFromGeoHash};
pub use geom_from_text::{
    GeomCollFromText, GeomFromEWKT, GeomFromText, LineFromText, MLineFromText, MPointFromText,
    MPolyFromText, PointFromText, PolygonFromText,
};
pub use geom_from_wkb::{GeomFromEWKB, GeomFromWKB};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsBinary.into());
    session_context.register_udf(AsEWKB.into());
    session_context.register_udf(AsHEXEWKB.into());
    session_context.register_udf(GeomFromWKB::default().into());
    session_context.register_udf(GeomFromEWKB.into());
    session_context.register_udf(AsText.into());
    session_context.register_udf(AsEWKT.into());
    session_context.register_udf(GeoHash.into());
    session_context.register_udf(PointFromGeoHash.into());
    session_context.register_udf(GeomFromGeoHash.into());
    session_context.register_udf(Box2DFromGeoHash.into());
    session_context.register_udf(GeomFromText::default().into());
    session_context.register_udf(GeomFromEWKT.into());
    session_context.register_udf(PointFromText.into());
    session_context.register_udf(LineFromText.into());
    session_context.register_udf(PolygonFromText::default().into());
    session_context.register_udf(MPointFromText::default().into());
    session_context.register_udf(MLineFromText::default().into());
    session_context.register_udf(MPolyFromText::default().into());
    session_context.register_udf(GeomCollFromText.into());
}
