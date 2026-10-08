use std::sync::Arc;

use arrow_schema::{Field, FieldRef};
use datafusion::common::config::ConfigOptions;
use datafusion::common::tree_node::{Transformed, TreeNodeRecursion};
use datafusion::common::{DFSchema, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::{ExprSchemable, LogicalPlan, Values};
use datafusion::optimizer::AnalyzerRule;
use geoarrow_schema::Metadata;

use crate::util::srid::crs_to_srid;

/// Gives a `VALUES` list's geometry columns the CRS of their values.
///
/// A `VALUES` list keeps the schema planned from its expressions, so the CRS that
/// [`GeoCastRewrite`](super::casts::GeoCastRewrite) finds in an `'SRID=4326;...'::geometry`
/// literal doesn't reach it: the column would have no CRS. This recomputes the geometry columns
/// of every `VALUES` list from its rows, and then the schemas above them.
///
/// Rows without an SRID take the column's, as in a PostGIS column with a type modifier. Rows with
/// different SRIDs are a plan error, because geodatafusion stores one CRS per column.
#[derive(Debug)]
pub(crate) struct GeoValuesSchema;

impl AnalyzerRule for GeoValuesSchema {
    fn name(&self) -> &str {
        "geodatafusion_values_schema"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> Result<LogicalPlan> {
        if !has_geometry_values(&plan)? {
            return Ok(plan);
        }
        plan.transform_up_with_subqueries(|plan| {
            let plan = match plan {
                LogicalPlan::Values(values) => LogicalPlan::Values(values_with_crs(values)?),
                // The schemas above a changed list follow it.
                plan => plan.recompute_schema()?,
            };
            Ok(Transformed::yes(plan))
        })
        .map(|transformed| transformed.data)
    }
}

fn has_geometry_values(plan: &LogicalPlan) -> Result<bool> {
    let mut found = false;
    plan.apply_with_subqueries(|plan| {
        found = matches!(plan, LogicalPlan::Values(Values { schema, .. })
            if schema.fields().iter().any(|field| is_spatial(field)));
        Ok(if found {
            TreeNodeRecursion::Stop
        } else {
            TreeNodeRecursion::Continue
        })
    })?;
    Ok(found)
}

fn values_with_crs(values: Values) -> Result<Values> {
    let Values { schema, values } = values;
    let empty = DFSchema::empty();
    let fields = schema
        .iter()
        .enumerate()
        .map(|(column, (qualifier, field))| {
            if !is_spatial(field) {
                return Ok((qualifier.cloned(), Arc::clone(field)));
            }
            let mut chosen: Option<(FieldRef, Option<i32>)> = None;
            for row in &values {
                let (_, value_field) = row[column].to_field(&empty)?;
                if value_field.extension_type_name() != field.extension_type_name() {
                    continue;
                }
                let metadata = Metadata::try_from(value_field.as_ref()).unwrap_or_default();
                if metadata.crs() == &Default::default() {
                    continue;
                }
                let srid = crs_to_srid(metadata.crs());
                match &chosen {
                    None => chosen = Some((value_field, srid)),
                    Some((chosen_field, chosen_srid)) => {
                        let same = match (chosen_srid, srid) {
                            (Some(a), Some(b)) => *a == b,
                            _ => chosen_field.metadata() == value_field.metadata(),
                        };
                        if !same {
                            return plan_err!(
                                "VALUES: Operation on mixed SRID geometries in column {}; \
                                 geodatafusion stores one SRID per column",
                                field.name()
                            );
                        }
                    }
                }
            }
            let field = match chosen {
                Some((value_field, _)) => {
                    Arc::new(Field::clone(field).with_metadata(value_field.metadata().clone()))
                }
                None => Arc::clone(field),
            };
            Ok((qualifier.cloned(), field))
        })
        .collect::<Result<Vec<_>>>()?;
    let schema = Arc::new(DFSchema::new_with_metadata(
        fields,
        schema.metadata().clone(),
    )?);
    Ok(Values { schema, values })
}

fn is_spatial(field: &Field) -> bool {
    field
        .extension_type_name()
        .is_some_and(|name| name.starts_with("geoarrow."))
}
