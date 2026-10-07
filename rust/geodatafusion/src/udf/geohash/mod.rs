mod box2d_from_geohash;
mod point_from_geohash;

pub use box2d_from_geohash::Box2DFromGeoHash;
pub use point_from_geohash::PointFromGeoHash;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Box2DFromGeoHash.into());
    session_context.register_udf(PointFromGeoHash::default().into());
}
