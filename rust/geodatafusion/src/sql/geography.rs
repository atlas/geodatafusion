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

use crate::util::geography::{geography_spheroid, geography_srid, is_geography};

/// The functions that take geography, PostGIS's geography overloads that geodatafusion
/// implements, and the conversions that the casts call.
///
/// A function joins this list when it computes on the sphere or spheroid for geography; until
/// then, geography is an error rather than a planar result in degrees.
pub const GEOGRAPHY_FUNCTIONS: &[&str] = &[
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
/// PostgreSQL does for PostGIS's: `ST_X(geography)` doesn't exist. Planar functions would
/// otherwise compute in degrees on longitude/latitude, without an error.
///
/// Also rejects geography in an SRID that isn't longitude/latitude, such as `geography(x)` of a
/// projected geometry, which PostGIS refuses: a function's return field states the CRS of what
/// it converts unchecked, so the unanalysed plan says what the statement does.
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
    check_lon_lat(expr, schema)?;
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

/// Refuses a function whose result is geography in an SRID that isn't longitude/latitude.
fn check_lon_lat(expr: &Expr, schema: &DFSchema) -> Result<()> {
    let Expr::ScalarFunction(ScalarFunction { func, .. }) = expr else {
        return Ok(());
    };
    let (_, field) = expr.to_field(schema)?;
    if !is_geography(&field) {
        return Ok(());
    }
    if let Some(srid) = geography_srid(&field) {
        geography_spheroid(func.name(), srid)?;
    }
    Ok(())
}

/// Whether a function is one of geodatafusion's, by PostGIS's naming.
fn is_geodatafusion(name: &str) -> bool {
    name.starts_with("st_")
        || name.starts_with("geometry_")
        || matches!(
            name,
            "box2d" | "box3d" | "box2d_out" | "box3d_out" | "geometrytype"
        )
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use datafusion::execution::session_state::SessionStateBuilder;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::sql::GeoTypePlanner;
    use crate::util::geography::geography_srid;

    const LON_LAT_ONLY: &str = "Only lon/lat coordinate systems are supported in geography";

    fn context() -> SessionContext {
        let state = SessionStateBuilder::new()
            .with_default_features()
            .with_type_planner(Arc::new(GeoTypePlanner::new()))
            .build();
        let ctx = SessionContext::new_with_state(state);
        crate::register(&ctx);
        ctx
    }

    /// The unanalysed plan states the CRS geography is taken of, so a caller reading it sees
    /// what the statement does; the plan is refused once it is analysed, as PostGIS refuses it.
    #[tokio::test]
    async fn test_geography_of_projected_geometry_plans_with_its_crs_and_is_refused_when_analysed()
    {
        let ctx = context();
        let called = ctx
            .sql("SELECT geography(ST_SetSRID(ST_Point(500000, 6600000), 25833)) AS g")
            .await
            .unwrap();
        let field = called.schema().field(0);
        assert!(is_geography(field));
        assert_eq!(geography_srid(field), Some(25833));
        let cast = ctx
            .sql("SELECT ST_SetSRID(ST_Point(500000, 6600000), 25833)::geography AS g")
            .await
            .unwrap();
        for frame in [called, cast] {
            let error = frame.collect().await.unwrap_err().to_string();
            assert!(
                error.contains(&format!("geography: {LON_LAT_ONLY} (SRID 25833)")),
                "{error}"
            );
        }
        let lon_lat = ctx
            .sql("SELECT geography(ST_Point(10.75, 59.91, 4258)) AS g")
            .await
            .unwrap();
        lon_lat.collect().await.unwrap();
    }

    /// Geometry intersected with geography is taken as geography in its own SRID, which is
    /// refused once the plan is analysed when it isn't longitude/latitude.
    #[tokio::test]
    async fn test_intersection_of_projected_geometry_with_geography_is_refused_when_analysed() {
        let ctx = context();
        let frame = ctx
            .sql(
                "SELECT ST_Intersection(ST_SetSRID(ST_Point(500000, 6600000), 25833), \
                 geography(ST_Point(10, 60))) AS g",
            )
            .await
            .unwrap();
        assert_eq!(geography_srid(frame.schema().field(0)), Some(25833));
        let error = frame.collect().await.unwrap_err().to_string();
        assert!(
            error.contains(&format!("st_intersection: {LON_LAT_ONLY} (SRID 25833)")),
            "{error}"
        );
    }
}
