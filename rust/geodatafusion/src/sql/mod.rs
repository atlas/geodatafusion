//! PostGIS's SQL types, the casts between them and its operators, behind the `sql` feature.

mod casts;
mod inserts;
mod operators;
mod types;
mod values;

use std::sync::Arc;

use datafusion::execution::FunctionRegistry;
use datafusion::prelude::SessionContext;
pub use types::GeoTypePlanner;

/// Adds the casts to and from the spatial types, including `INSERT`'s, and the operators. The
/// types themselves come from [`GeoTypePlanner`], which is set when the session is built.
pub(crate) fn register(session_context: &SessionContext) {
    let state = session_context.state_ref();
    let mut state = state.write();
    state
        .register_function_rewrite(Arc::new(casts::GeoCastRewrite))
        .expect("SessionState accepts function rewrites");
    state
        .register_expr_planner(Arc::new(operators::GeoExprPlanner))
        .expect("SessionState accepts expression planners");
    drop(state);
    session_context.add_analyzer_rule(Arc::new(inserts::GeoInsertCasts));
    session_context.add_analyzer_rule(Arc::new(values::GeoValuesSchema));
}
