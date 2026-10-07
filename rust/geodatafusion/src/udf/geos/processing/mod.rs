mod build_area;
mod line_merge;
mod reduce_precision;

pub use build_area::BuildArea;
pub use line_merge::LineMerge;
pub use reduce_precision::ReducePrecision;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(BuildArea.into());
    session_context.register_udf(LineMerge.into());
    session_context.register_udf(ReducePrecision.into());
}
