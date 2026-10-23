mod transform;
mod transform_pipeline;

pub use transform::Transform;
#[cfg(feature = "geos-3_11")]
pub(crate) use transform::transform_geometry;
pub use transform_pipeline::{InverseTransformPipeline, TransformPipeline};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Transform.into());
    session_context.register_udf(TransformPipeline.into());
    session_context.register_udf(InverseTransformPipeline.into());
}
