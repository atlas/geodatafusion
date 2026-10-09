//! Measurement functions.

mod area;
pub(crate) mod distance;
mod length;
pub(crate) mod length3d;
mod perimeter;

pub use area::Area;
pub use distance::Distance;
pub use length::Length;
pub use length3d::{Length3D, Perimeter3D};
pub use perimeter::Perimeter;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Area.into());
    session_context.register_udf(Distance::new().into());
    session_context.register_udf(Length.into());
    session_context.register_udf(Length3D.into());
    session_context.register_udf(Perimeter3D.into());
    session_context.register_udf(Perimeter.into());
}
