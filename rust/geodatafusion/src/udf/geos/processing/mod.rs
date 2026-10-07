mod buffer;
mod build_area;
mod line_merge;
mod offset_curve;
mod reduce_precision;

pub use buffer::Buffer;
pub use build_area::BuildArea;
pub use line_merge::LineMerge;
pub use offset_curve::OffsetCurve;
pub use reduce_precision::ReducePrecision;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Buffer.into());
    session_context.register_udf(BuildArea.into());
    session_context.register_udf(LineMerge.into());
    session_context.register_udf(OffsetCurve.into());
    session_context.register_udf(ReducePrecision.into());
}
