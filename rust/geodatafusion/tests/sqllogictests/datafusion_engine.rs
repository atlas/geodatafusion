//! A sqllogictest engine backed by DataFusion with all geodatafusion UDFs registered.

use std::sync::Arc;
use std::time::Duration;

use arrow_array::cast::AsArray;
use arrow_array::types::*;
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType, Field};
use datafusion::arrow::util::display::array_value_to_string;
use datafusion::execution::SessionStateBuilder;
use datafusion::prelude::SessionContext;
use geoarrow_array::array::from_arrow_array;
use geoarrow_array::cast::{AsGeoArrowArray, to_wkb};
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::GeoArrowType;
use geoarrow_schema::crs::CrsType;
use geodatafusion::sql::GeoTypePlanner;
use sqllogictest::{AsyncDB, DBOutput, DefaultColumnType};

use crate::{EngineError, render};

/// Maximum time a single record may take.
const RECORD_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GeoDataFusion {
    ctx: SessionContext,
}

impl GeoDataFusion {
    pub fn new() -> Self {
        let state = SessionStateBuilder::new()
            .with_default_features()
            .with_type_planner(Arc::new(GeoTypePlanner::new()))
            .build();
        let ctx = SessionContext::new_with_state(state);
        geodatafusion::register(&ctx);
        Self { ctx }
    }

    async fn run_inner(&mut self, sql: &str) -> Result<DBOutput<DefaultColumnType>, EngineError> {
        let df = self.ctx.sql(sql).await?;
        let schema = df.schema().inner().clone();
        let batches = df.collect().await?;

        if schema.fields().is_empty() || is_statement(sql) {
            return Ok(DBOutput::StatementComplete(0));
        }

        let mut rows = vec![];
        for batch in &batches {
            rows.extend(render_batch(batch)?);
        }
        Ok(DBOutput::Rows {
            types: schema
                .fields()
                .iter()
                .map(|f| column_type(f.data_type()))
                .collect(),
            rows,
        })
    }
}

fn is_statement(sql: &str) -> bool {
    let first = sql
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(first.as_str(), "create" | "insert" | "drop")
}

fn column_type(dt: &DataType) -> DefaultColumnType {
    if dt.is_integer() {
        DefaultColumnType::Integer
    } else if dt.is_floating() || matches!(dt, DataType::Decimal128(..) | DataType::Decimal256(..))
    {
        DefaultColumnType::FloatingPoint
    } else {
        DefaultColumnType::Text
    }
}

fn render_batch(batch: &RecordBatch) -> Result<Vec<Vec<String>>, EngineError> {
    let schema = batch.schema();
    let columns = batch
        .columns()
        .iter()
        .zip(schema.fields())
        .map(|(array, field)| render_column(array.as_ref(), field))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((0..batch.num_rows())
        .map(|row| columns.iter().map(|c| c[row].clone()).collect())
        .collect())
}

fn render_column(array: &dyn Array, field: &Field) -> Result<Vec<String>, EngineError> {
    if field
        .extension_type_name()
        .is_some_and(|name| name.starts_with("geoarrow."))
    {
        return render_geometry_column(array, field);
    }
    (0..array.len())
        .map(|i| render_value(array, field, i))
        .collect()
}

fn render_geometry_column(array: &dyn Array, field: &Field) -> Result<Vec<String>, EngineError> {
    let geo_array = from_arrow_array(array, field)?;
    let metadata = geo_array.data_type().metadata().clone();
    let srid = srid_from_crs(&metadata);

    if let GeoArrowType::Rect(_) = geo_array.data_type() {
        let rects = geo_array.as_rect();
        return (0..rects.len())
            .map(|i| {
                Ok(match rects.get(i)? {
                    None => render::NULL.to_string(),
                    Some(r) => render::rect(&r),
                })
            })
            .collect();
    }

    let wkb_array = to_wkb::<i32>(geo_array.as_ref())?;
    let binary = wkb_array.to_array_ref();
    let binary = binary.as_binary::<i32>();
    Ok((0..binary.len())
        .map(|i| {
            if binary.is_null(i) {
                render::NULL.to_string()
            } else {
                render::ewkb(binary.value(i), srid)
            }
        })
        .collect())
}

fn srid_from_crs(metadata: &geoarrow_schema::Metadata) -> Option<i32> {
    let crs = metadata.crs();
    let value = crs.crs_value()?.as_str()?;
    match crs.crs_type()? {
        // PostGIS SRIDs are EPSG or ESRI codes; the harness doesn't share util::srid, so that it
        // doesn't share code with what it checks.
        CrsType::AuthorityCode => value
            .strip_prefix("EPSG:")
            .or_else(|| value.strip_prefix("ESRI:"))
            .and_then(|code| code.parse().ok()),
        CrsType::Srid => value.parse().ok(),
        _ => None,
    }
}

fn render_value(array: &dyn Array, field: &Field, i: usize) -> Result<String, EngineError> {
    if array.is_null(i) || array.data_type() == &DataType::Null {
        return Ok(render::NULL.to_string());
    }
    Ok(match array.data_type() {
        DataType::Boolean => array.as_boolean().value(i).to_string(),
        DataType::Float16 => render::float(array.as_primitive::<Float16Type>().value(i).to_f64()),
        DataType::Float32 => render::float(array.as_primitive::<Float32Type>().value(i) as f64),
        DataType::Float64 => render::float(array.as_primitive::<Float64Type>().value(i)),
        DataType::Decimal128(..) | DataType::Decimal256(..) => {
            let s = array_value_to_string(array, i)?;
            s.parse::<f64>()
                .map(render::float)
                .unwrap_or_else(|_| render::text(&s))
        }
        DataType::Utf8 => render::text(array.as_string::<i32>().value(i)),
        DataType::LargeUtf8 => render::text(array.as_string::<i64>().value(i)),
        DataType::Utf8View => render::text(array.as_string_view().value(i)),
        DataType::Binary => render::bytes(array.as_binary::<i32>().value(i)),
        DataType::LargeBinary => render::bytes(array.as_binary::<i64>().value(i)),
        DataType::BinaryView => render::bytes(array.as_binary_view().value(i)),
        DataType::List(inner) => render_list(array.as_list::<i32>().value(i).as_ref(), inner)?,
        DataType::LargeList(inner) => render_list(array.as_list::<i64>().value(i).as_ref(), inner)?,
        _ => {
            let _ = field;
            render::text(&array_value_to_string(array, i)?)
        }
    })
}

fn render_list(values: &dyn Array, field: &Field) -> Result<String, EngineError> {
    let rendered = render_column(values, field)?;
    Ok(format!("{{{}}}", rendered.join(",")))
}

#[async_trait::async_trait]
impl AsyncDB for GeoDataFusion {
    type Error = EngineError;
    type ColumnType = DefaultColumnType;

    async fn run(&mut self, sql: &str) -> Result<DBOutput<Self::ColumnType>, Self::Error> {
        match tokio::time::timeout(RECORD_TIMEOUT, self.run_inner(sql)).await {
            Ok(result) => result,
            Err(_) => Err(EngineError(format!(
                "timed out after {}s",
                RECORD_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn shutdown(&mut self) {}

    fn engine_name(&self) -> &str {
        "geodatafusion"
    }

    async fn sleep(dur: Duration) {
        tokio::time::sleep(dur).await
    }
}
