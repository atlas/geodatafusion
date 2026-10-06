//! A sqllogictest engine backed by DataFusion with all geodatafusion UDFs registered.

use std::time::Duration;

use arrow_array::cast::AsArray;
use arrow_array::types::*;
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType, Field};
use datafusion::arrow::util::display::array_value_to_string;
use datafusion::prelude::SessionContext;
use geoarrow_array::array::from_arrow_array;
use geoarrow_array::cast::{AsGeoArrowArray, to_wkb};
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::GeoArrowType;
use geoarrow_schema::crs::CrsType;
use sqllogictest::{AsyncDB, DBOutput, DefaultColumnType};

use crate::{EngineError, render};

/// Maximum time a single record may take.
const RECORD_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GeoDataFusion {
    ctx: SessionContext,
}

impl GeoDataFusion {
    pub fn new() -> Self {
        let ctx = SessionContext::new();
        geodatafusion::register(&ctx);
        Self { ctx }
    }

    async fn run_inner(&mut self, sql: &str) -> Result<DBOutput<DefaultColumnType>, EngineError> {
        let sql = rewrite_geometry_literals(sql);
        let df = self.ctx.sql(&sql).await?;
        let schema = df.schema().inner().clone();
        let batches = df.collect().await?;

        if schema.fields().is_empty() || is_statement(&sql) {
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
    match field.extension_type_name() {
        // WKT columns are what PostGIS returns as `text` (e.g. from ST_AsText), so they are
        // compared verbatim: coordinate formatting is part of the behaviour under test.
        Some("geoarrow.wkt") => {}
        Some(name) if name.starts_with("geoarrow.") => {
            return render_geometry_column(array, field);
        }
        _ => {}
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

/// Rewrite Postgres-style geometry literals into function calls geodatafusion understands.
///
/// DataFusion has no `geometry` SQL type, so `'POINT(1 2)'::geometry` cannot be planned. This is a
/// known parity gap that is orthogonal to whether the *functions* behave like PostGIS, so the test
/// harness papers over it to keep the function tests meaningful:
///
/// - `'<wkt>'::geometry`          -> `ST_GeomFromText('<wkt>')`
/// - `'SRID=n;<wkt>'::geometry`   -> `ST_GeomFromEWKT('SRID=n;<wkt>')`
/// - `'<hex ewkb>'::geometry`     -> `ST_GeomFromEWKB(X'<hex>')`
/// - `'<wkt>'::geography`         -> `ST_GeogFromText('<wkt>')`
/// - `NULL::geometry`             -> `ST_GeomFromWKB(CAST(NULL AS BYTEA))`
///
/// `CAST('<lit>' AS geometry)` is handled the same way.
pub fn rewrite_geometry_literals(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < bytes.len() {
        // Copy line comments unchanged.
        if sql[i..].starts_with("--") {
            let end = sql[i..].find('\n').map_or(sql.len(), |n| i + n);
            out.push_str(&sql[i..end]);
            i = end;
            continue;
        }
        // Rewrite `CAST('<lit>' AS geometry)`.
        if sql
            .get(i..i + 4)
            .is_some_and(|s| s.eq_ignore_ascii_case("cast"))
            && !prev_is_ident(sql, i)
            && let Some((replacement, end)) = rewrite_cast(sql, i + 4)
        {
            out.push_str(&replacement);
            i = end;
            continue;
        }
        // Rewrite `NULL::geometry`.
        if sql
            .get(i..i + 4)
            .is_some_and(|s| s.eq_ignore_ascii_case("null"))
            && !prev_is_ident(sql, i)
        {
            let j = skip_ws(sql, i + 4);
            if sql[j..].starts_with("::")
                && let Some((_, type_end)) = geo_type_name(sql, skip_ws(sql, j + 2))
            {
                out.push_str("ST_GeomFromWKB(CAST(NULL AS BYTEA))");
                i = type_end;
                continue;
            }
        }
        if bytes[i] == b'\'' {
            let lit_end = string_literal_end(sql, i);
            let literal = &sql[i + 1..lit_end - 1];
            let mut j = skip_ws(sql, lit_end);
            if sql[j..].starts_with("::") {
                j = skip_ws(sql, j + 2);
                if let Some((type_name, type_end)) = geo_type_name(sql, j) {
                    out.push_str(&literal_to_call(literal, type_name));
                    i = type_end;
                    continue;
                }
            }
            out.push_str(&sql[i..lit_end]);
            i = lit_end;
            continue;
        }
        let ch = sql[i..].chars().next().expect("i < sql.len()");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn prev_is_ident(sql: &str, i: usize) -> bool {
    sql[..i]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn skip_ws(sql: &str, mut i: usize) -> usize {
    while i < sql.len() && sql.as_bytes()[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Given the index of an opening quote, return the index just past the closing quote.
fn string_literal_end(sql: &str, start: usize) -> usize {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

/// If `geometry` or `geography` (not followed by an identifier char or typmod) starts at `i`.
fn geo_type_name(sql: &str, i: usize) -> Option<(&'static str, usize)> {
    for name in ["geometry", "geography"] {
        let end = i + name.len();
        if sql
            .get(i..end)
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
        {
            let next = sql[end..].chars().next();
            if next.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '(')) {
                return Some((name, end));
            }
        }
    }
    None
}

fn rewrite_cast(sql: &str, after_cast: usize) -> Option<(String, usize)> {
    let mut j = skip_ws(sql, after_cast);
    if !sql[j..].starts_with('(') {
        return None;
    }
    j = skip_ws(sql, j + 1);
    if !sql[j..].starts_with('\'') {
        return None;
    }
    let lit_end = string_literal_end(sql, j);
    let literal = &sql[j + 1..lit_end - 1];
    j = skip_ws(sql, lit_end);
    if !sql
        .get(j..j + 2)
        .is_some_and(|s| s.eq_ignore_ascii_case("as"))
    {
        return None;
    }
    j = skip_ws(sql, j + 2);
    let (type_name, type_end) = geo_type_name(sql, j)?;
    j = skip_ws(sql, type_end);
    if !sql[j..].starts_with(')') {
        return None;
    }
    Some((literal_to_call(literal, type_name), j + 1))
}

fn literal_to_call(literal: &str, type_name: &str) -> String {
    let quoted = format!("'{literal}'");
    if type_name == "geography" {
        return format!("ST_GeogFromText({quoted})");
    }
    let trimmed = literal.trim();
    if !trimmed.is_empty()
        && trimmed.len().is_multiple_of(2)
        && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
    {
        format!("ST_GeomFromEWKB(X'{trimmed}')")
    } else if trimmed
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("srid="))
    {
        format!("ST_GeomFromEWKT({quoted})")
    } else {
        format!("ST_GeomFromText({quoted})")
    }
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
