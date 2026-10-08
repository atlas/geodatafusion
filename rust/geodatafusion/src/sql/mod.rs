//! PostGIS's SQL types and the casts between them, behind the `sql` feature.

mod casts;
mod inserts;
mod types;
mod values;

use std::sync::Arc;

use datafusion::execution::FunctionRegistry;
use datafusion::prelude::SessionContext;
pub use types::GeoTypePlanner;

/// Adds the casts to and from the spatial types, including `INSERT`'s. The types themselves come from
/// [`GeoTypePlanner`], which is set when the session is built.
pub(crate) fn register(session_context: &SessionContext) {
    session_context
        .state_ref()
        .write()
        .register_function_rewrite(Arc::new(casts::GeoCastRewrite))
        .expect("SessionState accepts function rewrites");
    session_context.add_analyzer_rule(Arc::new(inserts::GeoInsertCasts));
    session_context.add_analyzer_rule(Arc::new(values::GeoValuesSchema));
}
