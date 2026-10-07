//! Overlay functions.

mod binary_overlay;
mod node;
mod shared_paths;
mod unary_union;

pub use binary_overlay::{Difference, Intersection, SymDifference};
pub use node::Node;
pub use shared_paths::SharedPaths;
pub use unary_union::UnaryUnion;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Intersection.into());
    session_context.register_udf(Difference.into());
    session_context.register_udf(SymDifference.into());
    session_context.register_udf(UnaryUnion.into());
    session_context.register_udf(Node.into());
    session_context.register_udf(SharedPaths.into());
}
