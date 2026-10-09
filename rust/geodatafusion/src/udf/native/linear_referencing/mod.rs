mod line_interpolate_point;
mod line_interpolate_points;
mod line_substring;
pub(crate) mod util;

pub use line_interpolate_point::{LineInterpolatePoint, LineInterpolatePoint3D};
pub use line_interpolate_points::LineInterpolatePoints;
pub use line_substring::LineSubstring;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(LineInterpolatePoint.into());
    session_context.register_udf(LineInterpolatePoint3D.into());
    session_context.register_udf(LineInterpolatePoints.into());
    session_context.register_udf(LineSubstring.into());
}
