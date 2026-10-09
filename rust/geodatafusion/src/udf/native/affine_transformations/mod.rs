mod affine;
mod rotate;
mod scale;
mod translate;
pub(crate) mod util;

pub use affine::Affine;
pub use rotate::{Rotate, RotateX, RotateY, RotateZ};
pub use scale::Scale;
pub use translate::Translate;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Affine.into());
    session_context.register_udf(Translate.into());
    session_context.register_udf(Scale.into());
    session_context.register_udf(Rotate.into());
    session_context.register_udf(RotateX.into());
    session_context.register_udf(RotateY.into());
    session_context.register_udf(RotateZ.into());
}
