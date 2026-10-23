use arrow_schema::Field;
use datafusion::common::config::ConfigOptions;
use datafusion::common::tree_node::{TreeNode, TreeNodeRecursion};
use datafusion::common::{DFSchema, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::expr::{AggregateFunction, ScalarFunction, WindowFunction};
use datafusion::logical_expr::utils::merge_schema;
use datafusion::logical_expr::{Expr, ExprSchemable, LogicalPlan};
use datafusion::optimizer::AnalyzerRule;
use geoarrow_schema::{BoxType, Dimension};

use crate::util::geography::is_geography;

/// The functions that take geography, PostGIS's geography overloads that geodatafusion
/// implements, and the conversions that the casts call.
///
/// A function joins this list when it computes on the sphere or spheroid for geography; until
/// then, geography is an error rather than a planar result in degrees.
const GEOGRAPHY_FUNCTIONS: &[&str] = &[
    "geography",
    "geometry",
    "geometrytype",
    "st_area",
    "st_asbinary",
    "st_asewkb",
    "st_asewkt",
    "st_asgeojson",
    "st_ashexewkb",
    "st_assvg",
    "st_astext",
    "st_azimuth",
    "st_buffer",
    "st_centroid",
    "st_closestpoint",
    "st_coveredby",
    "st_covers",
    "st_distance",
    "st_dwithin",
    "st_geohash",
    "st_intersection",
    "st_intersects",
    "st_length",
    "st_lineinterpolatepoint",
    "st_lineinterpolatepoints",
    "st_linelocatepoint",
    "st_linesubstring",
    "st_perimeter",
    "st_project",
    "st_segmentize",
    "st_setsrid",
    "st_shortestline",
    "st_srid",
    "st_summary",
];

/// Rejects geography arguments to geodatafusion's functions that have no geography form, as
/// PostgreSQL does for PostGIS's: `ST_X(geography)` doesn't exist.
///
/// Planar functions would otherwise compute in degrees on longitude/latitude, without an error.
#[derive(Debug)]
pub(crate) struct GeoGeographyCalls;

impl AnalyzerRule for GeoGeographyCalls {
    fn name(&self) -> &str {
        "geodatafusion_geography_calls"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> Result<LogicalPlan> {
        plan.apply_with_subqueries(|node| {
            let inputs = node.inputs();
            let schema = if inputs.is_empty() {
                DFSchema::empty()
            } else {
                merge_schema(&inputs)
            };
            node.apply_expressions(|expr| {
                expr.apply(|expr| {
                    check_call(expr, &schema)?;
                    Ok(TreeNodeRecursion::Continue)
                })
            })
        })?;
        Ok(plan)
    }
}

fn check_call(expr: &Expr, schema: &DFSchema) -> Result<()> {
    let (name, args) = match expr {
        Expr::ScalarFunction(ScalarFunction { func, args }) => (func.name(), args),
        Expr::AggregateFunction(AggregateFunction { func, params }) => (func.name(), &params.args),
        Expr::WindowFunction(window) => {
            let WindowFunction { fun, params } = window.as_ref();
            (fun.name(), &params.args)
        }
        _ => return Ok(()),
    };
    let name = name.to_ascii_lowercase();
    if !is_geodatafusion(&name) || GEOGRAPHY_FUNCTIONS.contains(&name.as_str()) {
        return Ok(());
    }
    let fields = args
        .iter()
        .map(|arg| Ok(arg.to_field(schema)?.1))
        .collect::<Result<Vec<_>>>()?;
    if fields.iter().any(|field| is_geography(field)) {
        let types: Vec<_> = fields.iter().map(|field| sql_type(field)).collect();
        return plan_err!("function {name}({}) does not exist", types.join(", "));
    }
    Ok(())
}

/// Whether a function is one of geodatafusion's, by PostGIS's naming.
fn is_geodatafusion(name: &str) -> bool {
    name.starts_with("st_")
        || name.starts_with("geometry_")
        || matches!(name, "box2d" | "box3d" | "geometrytype")
}

/// The SQL type of an argument, for the error.
fn sql_type(field: &Field) -> String {
    if is_geography(field) {
        return "geography".to_string();
    }
    if let Ok(box_type) = field.try_extension_type::<BoxType>() {
        return match box_type.dimension() {
            Dimension::XYZ => "box3d".to_string(),
            _ => "box2d".to_string(),
        };
    }
    if field.extension_type_name().is_some() {
        return "geometry".to_string();
    }
    field.data_type().to_string().to_ascii_lowercase()
}
