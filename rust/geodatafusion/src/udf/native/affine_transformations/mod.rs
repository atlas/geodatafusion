mod affine;
mod scale;
mod translate;
pub(crate) mod util;

pub use affine::Affine;
pub use scale::Scale;
pub use translate::Translate;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Affine.into());
    session_context.register_udf(Translate.into());
    session_context.register_udf(Scale.into());
}
