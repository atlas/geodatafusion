mod box_operators;
mod distance_nd;
mod geography_operators;

pub use box_operators::{
    GeometryAbove, GeometryBelow, GeometryContains, GeometryDistanceBox, GeometryLeft,
    GeometryOverAbove, GeometryOverBelow, GeometryOverLeft, GeometryOverRight, GeometryOverlaps,
    GeometryOverlapsNd, GeometryRight, GeometrySame, GeometryWithin,
};
pub use distance_nd::GeometryDistanceCentroidNd;
pub use geography_operators::{GeographyDistanceKnn, GeographyOverlaps};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(GeographyDistanceKnn.into());
    session_context.register_udf(GeographyOverlaps.into());
    session_context.register_udf(GeometryAbove.into());
    session_context.register_udf(GeometryBelow.into());
    session_context.register_udf(GeometryContains.into());
    session_context.register_udf(GeometryDistanceBox.into());
    session_context.register_udf(GeometryDistanceCentroidNd.into());
    session_context.register_udf(GeometryLeft.into());
    session_context.register_udf(GeometryOverAbove.into());
    session_context.register_udf(GeometryOverBelow.into());
    session_context.register_udf(GeometryOverLeft.into());
    session_context.register_udf(GeometryOverRight.into());
    session_context.register_udf(GeometryOverlaps.into());
    session_context.register_udf(GeometryOverlapsNd.into());
    session_context.register_udf(GeometryRight.into());
    session_context.register_udf(GeometrySame.into());
    session_context.register_udf(GeometryWithin.into());
}
