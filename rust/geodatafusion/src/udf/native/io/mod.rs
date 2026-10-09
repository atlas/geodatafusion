//! Geometry Input and Output

mod as_binary;
mod as_encoded_polyline;
mod as_geojson;
mod as_text;
mod geo_hash;
mod geom_from_geo_hash;
mod geom_from_geojson;
mod geom_from_text;
mod geom_from_wkb;
mod line_from_encoded_polyline;
pub(crate) mod util;

pub use as_binary::{AsBinary, AsEWKB, AsHEXEWKB};
pub use as_encoded_polyline::AsEncodedPolyline;
pub use as_geojson::AsGeoJSON;
pub use as_text::{AsEWKT, AsText};
pub use geo_hash::GeoHash;
pub use geom_from_geo_hash::{Box2DFromGeoHash, GeomFromGeoHash, PointFromGeoHash};
pub use geom_from_geojson::GeomFromGeoJSON;
pub use geom_from_text::{
    GeomCollFromText, GeomFromEWKT, GeomFromText, LineFromText, MLineFromText, MPointFromText,
    MPolyFromText, PointFromText, PolygonFromText,
};
pub use geom_from_wkb::{
    GeomCollFromWKB, GeomFromEWKB, GeomFromWKB, LineFromWKB, MLineFromWKB, MPointFromWKB,
    MPolyFromWKB, PointFromWKB, PolyFromWKB,
};
pub use line_from_encoded_polyline::LineFromEncodedPolyline;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(AsBinary.into());
    session_context.register_udf(AsEWKB.into());
    session_context.register_udf(AsHEXEWKB.into());
    session_context.register_udf(GeomFromWKB::default().into());
    session_context.register_udf(GeomFromEWKB.into());
    session_context.register_udf(PointFromWKB.into());
    session_context.register_udf(LineFromWKB::default().into());
    session_context.register_udf(PolyFromWKB::default().into());
    session_context.register_udf(MPointFromWKB::default().into());
    session_context.register_udf(MLineFromWKB::default().into());
    session_context.register_udf(MPolyFromWKB::default().into());
    session_context.register_udf(GeomCollFromWKB.into());
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
    session_context.register_udf(AsGeoJSON.into());
    session_context.register_udf(GeomFromGeoJSON.into());
    session_context.register_udf(AsEncodedPolyline.into());
    session_context.register_udf(LineFromEncodedPolyline.into());
}
