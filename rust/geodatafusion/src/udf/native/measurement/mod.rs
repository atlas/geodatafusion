//! Measurement functions.

mod area;
mod azimuth;
mod distance;
mod distance3d;
mod length;
pub(crate) mod length3d;
mod longest_line;
mod perimeter;
mod shortest_line;
mod spheroid;

pub use area::Area;
pub use azimuth::Azimuth;
pub use distance::Distance;
pub use distance3d::{ClosestPoint3D, Distance3D, LongestLine3D, MaxDistance3D, ShortestLine3D};
pub use length::Length;
pub use length3d::{Length3D, Perimeter3D};
pub use longest_line::{LongestLine, MaxDistance};
pub use perimeter::Perimeter;
pub use shortest_line::{ClosestPoint, ShortestLine};
pub use spheroid::{DistanceSphere, DistanceSpheroid, LengthSpheroid};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Area.into());
    session_context.register_udf(Azimuth.into());
    session_context.register_udf(ClosestPoint.into());
    session_context.register_udf(ClosestPoint3D.into());
    session_context.register_udf(Distance::new().into());
    session_context.register_udf(Distance3D.into());
    session_context.register_udf(DistanceSphere.into());
    session_context.register_udf(DistanceSpheroid.into());
    session_context.register_udf(Length.into());
    session_context.register_udf(Length3D.into());
    session_context.register_udf(LengthSpheroid.into());
    session_context.register_udf(LongestLine.into());
    session_context.register_udf(LongestLine3D.into());
    session_context.register_udf(MaxDistance.into());
    session_context.register_udf(MaxDistance3D.into());
    session_context.register_udf(Perimeter3D.into());
    session_context.register_udf(Perimeter.into());
    session_context.register_udf(ShortestLine.into());
    session_context.register_udf(ShortestLine3D.into());
}
