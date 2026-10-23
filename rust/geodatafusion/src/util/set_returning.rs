//! Set-returning functions: a scalar UDF that returns `List<Struct<..>>` per row, also exposed as
//! a table function of the same name; and likewise functions that return one record.
//!
//! DataFusion can't call a function that returns rows in a `SELECT` list or laterally, so the
//! scalar form is the core: `unnest(ST_Dump(geom))` gives the rows of `ST_Dump(geom)`, and the
//! struct keeps the geometry's GeoArrow metadata, which `unnest` of a bare list would drop. The
//! table function makes PostgreSQL's `SELECT * FROM st_dump(<constant>)` work too.

use std::sync::Arc;

use datafusion::catalog::{TableFunctionArgs, TableFunctionImpl, TableProvider};
use datafusion::common::{Column, UnnestOptions};
use datafusion::datasource::ViewTable;
use datafusion::error::Result;
use datafusion::logical_expr::{Expr, LogicalPlanBuilder, ScalarUDF};
use datafusion::prelude::SessionContext;

/// The name of the column holding the scalar UDF's result in the table function's plan.
const RESULT_COLUMN: &str = "__set_returning";

/// A scalar UDF returning `List<Struct<..>>`, or a single `Struct<..>` record, as a table
/// function with a column per struct field.
#[derive(Debug)]
struct SetReturningTableFunction {
    udf: Arc<ScalarUDF>,
    /// Whether the UDF returns a list of records rather than one.
    list: bool,
}

impl TableFunctionImpl for SetReturningTableFunction {
    fn call_with_args(&self, args: TableFunctionArgs) -> Result<Arc<dyn TableProvider>> {
        let args = args.exprs().iter().cloned();
        #[cfg(feature = "sql")]
        let args = args.map(crate::sql::unfold_text_cast);
        let call = self.udf.call(args.collect()).alias(RESULT_COLUMN);
        // Unnesting twice turns the list into rows and then the struct into columns, named
        // `<column>.<field>`. A NULL list gives no rows, rather than a row of NULLs; a NULL
        // record gives a row of NULLs, as in PostgreSQL.
        let options = UnnestOptions::new().with_preserve_nulls(false);
        let mut plan = LogicalPlanBuilder::empty(true).project(vec![call])?;
        if self.list {
            plan = plan.unnest_column_with_options(RESULT_COLUMN, options.clone())?;
        }
        let plan = plan
            .unnest_column_with_options(RESULT_COLUMN, options)?
            .build()?;
        let prefix = format!("{RESULT_COLUMN}.");
        let columns: Vec<Expr> = plan
            .schema()
            .columns()
            .into_iter()
            .map(|column: Column| {
                let name = column.name.trim_start_matches(&prefix).to_string();
                Expr::Column(column).alias(name)
            })
            .collect();
        let plan = LogicalPlanBuilder::from(plan).project(columns)?.build()?;
        Ok(Arc::new(ViewTable::new(plan, None)))
    }
}

/// Registers a set-returning function: the scalar UDF, and a table function of the same name.
///
/// DataFusion doesn't fold the case of table function names, so `FROM st_dump(...)` works but
/// `FROM ST_Dump(...)` doesn't.
pub(crate) fn register_set_returning(session_context: &SessionContext, udf: ScalarUDF) {
    register(session_context, udf, true);
}

/// Registers a function returning one record: the scalar UDF, returning `Struct<..>`, and a table
/// function of the same name giving the record as a row.
#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_IsValidDetail, backed by GEOS, returns a record so far"
    )
)]
pub(crate) fn register_record(session_context: &SessionContext, udf: ScalarUDF) {
    register(session_context, udf, false);
}

fn register(session_context: &SessionContext, udf: ScalarUDF, list: bool) {
    let udf = Arc::new(udf);
    session_context.register_udtf(
        udf.name(),
        Arc::new(SetReturningTableFunction {
            udf: Arc::clone(&udf),
            list,
        }),
    );
    session_context.register_udf(Arc::unwrap_or_clone(udf));
}
