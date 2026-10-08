use std::sync::Arc;

use datafusion::common::DFSchema;
use datafusion::error::Result;
use datafusion::logical_expr::expr::ScalarFunction;
use datafusion::logical_expr::planner::{ExprPlanner, PlannerResult, RawBinaryExpr};
use datafusion::logical_expr::{Expr, ExprSchemable, ScalarUDF, ScalarUDFImpl};
use datafusion::sql::sqlparser::ast::BinaryOperator;

use crate::udf::native::measurement::Distance;
use crate::udf::native::operators::{
    GeometryAbove, GeometryBelow, GeometryContains, GeometryDistanceBox, GeometryLeft,
    GeometryOverAbove, GeometryOverBelow, GeometryOverLeft, GeometryOverRight, GeometryOverlaps,
    GeometryOverlapsNd, GeometryRight, GeometrySame, GeometryWithin,
};

/// Plans PostGIS's operators as calls of the functions behind them.
///
/// | Operator | Function |
/// |---|---|
/// | `&&`, `&&&` | `geometry_overlaps`, `geometry_overlaps_nd` |
/// | `~`, `@`, `~=` | `geometry_contains`, `geometry_within`, `geometry_same` |
/// | `<<`, `&<`, `>>`, `&>` | `geometry_left`, `geometry_overleft`, `geometry_right`, `geometry_overright` |
/// | `<<\|`, `&<\|`, `\|>>`, `\|&>` | `geometry_below`, `geometry_overbelow`, `geometry_above`, `geometry_overabove` |
/// | `<->`, `<#>` | `st_distance`, `geometry_distance_box` |
///
/// An operator is planned only when an operand is a geometry or a box, so the operators DataFusion
/// already has (`&&` on arrays, `~` on strings, `<<` on integers) keep their meaning. Only the
/// PostgreSQL dialect parses most of them: `SET datafusion.sql_parser.dialect = 'PostgreSQL'`.
#[derive(Debug)]
pub(crate) struct GeoExprPlanner;

impl ExprPlanner for GeoExprPlanner {
    fn plan_binary_op(
        &self,
        expr: RawBinaryExpr,
        schema: &DFSchema,
    ) -> Result<PlannerResult<RawBinaryExpr>> {
        let Some(function) = operator_function(&expr.op) else {
            return Ok(PlannerResult::Original(expr));
        };
        if !is_spatial(&expr.left, schema)? && !is_spatial(&expr.right, schema)? {
            return Ok(PlannerResult::Original(expr));
        }
        Ok(PlannerResult::Planned(Expr::ScalarFunction(
            ScalarFunction::new_udf(function, vec![expr.left, expr.right]),
        )))
    }
}

fn operator_function(op: &BinaryOperator) -> Option<Arc<ScalarUDF>> {
    use BinaryOperator::*;

    Some(match op {
        PGOverlap => udf(GeometryOverlaps),
        Custom(op) if op == "&&&" => udf(GeometryOverlapsNd),
        PGRegexMatch => udf(GeometryContains),
        At => udf(GeometryWithin),
        TildeEq => udf(GeometrySame),
        PGBitwiseShiftLeft => udf(GeometryLeft),
        AndLt => udf(GeometryOverLeft),
        PGBitwiseShiftRight => udf(GeometryRight),
        AndGt => udf(GeometryOverRight),
        LtLtPipe => udf(GeometryBelow),
        AndLtPipe => udf(GeometryOverBelow),
        PipeGtGt => udf(GeometryAbove),
        PipeAndGt => udf(GeometryOverAbove),
        LtDashGt => udf(Distance::new()),
        Custom(op) if op == "<#>" => udf(GeometryDistanceBox),
        _ => return None,
    })
}

fn is_spatial(expr: &Expr, schema: &DFSchema) -> Result<bool> {
    let (_, field) = expr.to_field(schema)?;
    Ok(field
        .extension_type_name()
        .is_some_and(|name| name.starts_with("geoarrow.")))
}

fn udf(function: impl ScalarUDFImpl + 'static) -> Arc<ScalarUDF> {
    Arc::new(ScalarUDF::new_from_impl(function))
}
