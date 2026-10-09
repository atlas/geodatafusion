mod line_interpolate_point;
mod line_interpolate_points;
pub(crate) mod util;

pub use line_interpolate_point::{LineInterpolatePoint, LineInterpolatePoint3D};
pub use line_interpolate_points::LineInterpolatePoints;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(LineInterpolatePoint.into());
    session_context.register_udf(LineInterpolatePoint3D.into());
    session_context.register_udf(LineInterpolatePoints.into());
}
