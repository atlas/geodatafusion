//! Overlay functions.

mod binary_overlay;
mod node;
mod shared_paths;
mod unary_union;
mod union_agg;

pub use binary_overlay::{Difference, Intersection, SymDifference, Union};
pub use node::Node;
pub use shared_paths::SharedPaths;
pub use unary_union::UnaryUnion;
pub use union_agg::{MemUnion, UnionAgg};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Intersection.into());
    session_context.register_udf(Difference.into());
    session_context.register_udf(SymDifference.into());
    session_context.register_udf(Union.into());
    session_context.register_udaf(UnionAgg.into());
    session_context.register_udaf(MemUnion.into());
    session_context.register_udf(UnaryUnion.into());
    session_context.register_udf(Node.into());
    session_context.register_udf(SharedPaths.into());
}
