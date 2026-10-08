use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::config::ConfigOptions;
use datafusion::common::{DFSchema, Result};
use datafusion::logical_expr::expr::{Cast, ScalarFunction};
use datafusion::logical_expr::{
    DmlStatement, Expr, ExprSchemable, LogicalPlan, Projection, ScalarUDF, Values, WriteOp,
};
use datafusion::optimizer::AnalyzerRule;
use geoarrow_schema::WkbType;

use crate::udf::native::types::Geometry;
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// Converts the values an `INSERT` writes to a geometry column, as PostgreSQL's assignment casts
/// do.
///
/// DataFusion casts each inserted value to the column's storage type only, so
/// `INSERT INTO t VALUES ('POINT(1 2)')` would store the text's bytes as WKB. This replaces the
/// value with `geometry(value)` for the column's type modifier: text is parsed, a geometry
/// without an SRID takes the column's, and one with another SRID is an error.
#[derive(Debug)]
pub(crate) struct GeoInsertCasts;

impl AnalyzerRule for GeoInsertCasts {
    fn name(&self) -> &str {
        "geodatafusion_insert_casts"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> Result<LogicalPlan> {
        let LogicalPlan::Dml(dml) = plan else {
            return Ok(plan);
        };
        if !matches!(dml.op, WriteOp::Insert(_)) {
            return Ok(LogicalPlan::Dml(dml));
        }
        let table_schema = dml.target.schema();
        let mut input = Arc::unwrap_or_clone(dml.input);
        for (column, field) in table_schema.fields().iter().enumerate() {
            if let Ok(wkb_type) = field.try_extension_type::<WkbType>() {
                let srid = crs_to_srid(wkb_type.metadata().crs()).unwrap_or(SRID_UNKNOWN);
                input = convert_column(input, column, srid)?;
            }
        }
        Ok(LogicalPlan::Dml(DmlStatement::new(
            dml.table_name,
            dml.target,
            dml.op,
            Arc::new(input),
        )))
    }
}

/// Converts output column `column` of `plan` to a geometry with the SRID `srid`, where the value
/// is computed: in a projection, or in the rows of a `VALUES` list.
fn convert_column(plan: LogicalPlan, column: usize, srid: i32) -> Result<LogicalPlan> {
    match plan {
        LogicalPlan::Projection(projection) => {
            let Projection {
                mut expr, input, ..
            } = projection;
            let (name, value) = match &expr[column] {
                Expr::Alias(alias) => (Some(alias.name.clone()), alias.expr.as_ref().clone()),
                value => (None, value.clone()),
            };
            // `INSERT ... VALUES` projects the list's columns under the table's names.
            if let Expr::Column(source) = &value
                && let LogicalPlan::Projection(_) | LogicalPlan::Values(_) = input.as_ref()
            {
                let index = input.schema().index_of_column(source)?;
                let input = convert_column(Arc::unwrap_or_clone(input), index, srid)?;
                return Projection::try_new(expr, Arc::new(input)).map(LogicalPlan::Projection);
            }
            let converted = convert(value, input.schema(), srid)?;
            expr[column] = match name {
                Some(name) => converted.alias(name),
                None => converted,
            };
            Projection::try_new(expr, input).map(LogicalPlan::Projection)
        }
        LogicalPlan::Values(Values { schema, mut values }) => {
            let empty = DFSchema::empty();
            for row in &mut values {
                row[column] = convert(std::mem::take(&mut row[column]), &empty, srid)?;
            }
            let (_, converted) = values[0][column].to_field(&empty)?;
            let fields = schema
                .iter()
                .enumerate()
                .map(|(index, (qualifier, field))| {
                    let field = if index == column {
                        Arc::new(Field::clone(&converted).with_name(field.name()))
                    } else {
                        Arc::clone(field)
                    };
                    (qualifier.cloned(), field)
                })
                .collect();
            let schema = Arc::new(DFSchema::new_with_metadata(
                fields,
                schema.metadata().clone(),
            )?);
            Ok(LogicalPlan::Values(Values { schema, values }))
        }
        plan => {
            // Anything else, such as a table scan, gets a projection that converts the column.
            let schema = Arc::clone(plan.schema());
            let expr = schema
                .columns()
                .into_iter()
                .enumerate()
                .map(|(index, source)| {
                    let value = Expr::Column(source.clone());
                    if index == column {
                        Ok(convert(value, &schema, srid)?.alias(source.name()))
                    } else {
                        Ok(value)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            Projection::try_new(expr, Arc::new(plan)).map(LogicalPlan::Projection)
        }
    }
}

/// `geometry(value)` for a column with the SRID `srid`. DataFusion's cast to the storage type
/// is dropped: a cast of text to `Binary` is its bytes, not a geometry.
fn convert(value: Expr, schema: &DFSchema, srid: i32) -> Result<Expr> {
    let value = match value {
        Expr::Cast(Cast { expr, field })
            if field.extension_type_name().is_none() && is_storage_cast(&expr, schema)? =>
        {
            *expr
        }
        value => value,
    };
    let geometry = Arc::new(ScalarUDF::new_from_impl(Geometry::with_srid(srid)));
    Ok(Expr::ScalarFunction(ScalarFunction::new_udf(
        geometry,
        vec![value],
    )))
}

/// Whether DataFusion added the cast to store `value` in a `Binary` column: `value` is text, or
/// untagged bytes in another binary type.
fn is_storage_cast(value: &Expr, schema: &DFSchema) -> Result<bool> {
    let (_, field) = value.to_field(schema)?;
    Ok(field.extension_type_name().is_none()
        && matches!(
            field.data_type(),
            DataType::Utf8
                | DataType::LargeUtf8
                | DataType::Utf8View
                | DataType::Binary
                | DataType::LargeBinary
                | DataType::BinaryView
                | DataType::Null
        ))
}
