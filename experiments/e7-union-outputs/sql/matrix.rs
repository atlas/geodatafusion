//! E7 H2e: which SQL constructs keep extension metadata (`ARROW:extension:name` and
//! `ARROW:extension:metadata`, with a CRS) on a `geoarrow.geometry` (dense union) or
//! `geoarrow.wkb` (Binary) field. Compiled twice: against DataFusion 54 + geoarrow 0.8 (arrow 58)
//! and DataFusion 55 + geoarrow 0.9 (arrow 59). Prints one TSV row per case.
//!
//! For each case two things are recorded:
//! - `out`: the extension name/metadata on the query's output field (what a client sees);
//! - `udf`: the extension name/metadata a UDF applied on top sees in `arg_fields`
//!   (what geodatafusion sees: `meta_of(x)` over the construct as a subquery).

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use datafusion::common::ScalarValue;
use datafusion::datasource::MemTable;
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::{
    ColumnarValue, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion::prelude::SessionContext;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::{GeometryBuilder, WkbBuilder};
use geoarrow_schema::{CoordType, Crs, GeometryType, Metadata, WkbType};

const EXT_NAME: &str = "ARROW:extension:name";
const EXT_META: &str = "ARROW:extension:metadata";

fn meta() -> Arc<Metadata> {
    Arc::new(Metadata::new(Crs::from_authority_code("EPSG:4326".to_string()), None))
}

fn union_type() -> GeometryType {
    GeometryType::new(meta()).with_coord_type(CoordType::Separated)
}

fn wkb_type() -> WkbType {
    WkbType::new(meta())
}

fn ext(e: impl std::error::Error + Send + Sync + 'static) -> DataFusionError {
    DataFusionError::External(Box::new(e))
}

/// Points (i, i) for each input integer, as union or WKB, with the EPSG:4326 metadata.
fn build(kind: &str, ids: &[Option<i64>]) -> Result<(Field, ArrayRef)> {
    let pts: Vec<Option<geo_types::Point>> =
        ids.iter().map(|i| i.map(|i| geo_types::Point::new(i as f64, i as f64))).collect();
    Ok(match kind {
        "union" => {
            let mut b = GeometryBuilder::new(union_type());
            for p in &pts {
                b.push_geometry(p.as_ref()).map_err(ext)?;
            }
            let a = b.finish();
            (a.data_type().to_field("g", true), a.into_array_ref())
        }
        "plain" => {
            // Control: the same WKB bytes as plain Binary, no extension metadata.
            let (_, a) = build("wkb", ids)?;
            let f = Field::new("b", DataType::Binary, true);
            let a = a.as_any().downcast_ref::<arrow_array::BinaryArray>().cloned();
            let a = a.ok_or_else(|| DataFusionError::Internal("wkb".into()))?;
            (f, Arc::new(a) as ArrayRef)
        }
        _ => {
            let mut b = WkbBuilder::<i32>::new(wkb_type());
            for p in &pts {
                b.push_geometry(p.as_ref()).map_err(ext)?;
            }
            let a = b.finish();
            (a.data_type().to_field("w", true), a.into_array_ref())
        }
    })
}

/// `mk_union(id)` / `mk_wkb(id)`: a geometry-returning UDF with an extension-typed return field.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Mk {
    kind: &'static str,
    name: String,
    signature: Signature,
}

impl ScalarUDFImpl for Mk {
    fn name(&self) -> &str {
        &self.name
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, _: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("use return_field_from_args".into()))
    }
    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(Arc::new(build(self.kind, &[])?.0.with_name(self.name.clone())))
    }
    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let arr = args.args[0].clone().into_array(args.number_rows)?;
        let ids: Vec<Option<i64>> = arr
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| DataFusionError::Internal("mk: Int64 argument".into()))?
            .iter()
            .collect();
        let (_, out) = build(self.kind, &ids)?;
        Ok(match &args.args[0] {
            ColumnarValue::Scalar(_) => ColumnarValue::Scalar(ScalarValue::try_from_array(&out, 0)?),
            ColumnarValue::Array(_) => ColumnarValue::Array(out),
        })
    }
}

/// `meta_of(x)`: the extension name and metadata in the UDF's argument field, as text.
#[derive(Debug, PartialEq, Eq, Hash)]
struct MetaOf {
    signature: Signature,
}

fn describe(md: &HashMap<String, String>) -> String {
    match md.get(EXT_NAME) {
        None => "-".to_string(),
        Some(n) => format!("{n} {}", md.get(EXT_META).map(String::as_str).unwrap_or("<no metadata>")),
    }
}

impl ScalarUDFImpl for MetaOf {
    fn name(&self) -> &str {
        "meta_of"
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, _: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }
    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let f = &args.arg_fields[0];
        let s = format!("{} | {}", short(f.data_type()), describe(f.metadata()));
        Ok(ColumnarValue::Scalar(ScalarValue::Utf8(Some(s))))
    }
}

fn context() -> Result<SessionContext> {
    let ctx = SessionContext::new();
    for kind in ["union", "wkb"] {
        ctx.register_udf(ScalarUDF::from(Mk {
            kind,
            name: format!("mk_{kind}"),
            signature: Signature::exact(vec![DataType::Int64], Volatility::Immutable),
        }));
    }
    ctx.register_udf(ScalarUDF::from(MetaOf { signature: Signature::any(1, Volatility::Immutable) }));
    let ids = [Some(1i64), Some(2)];
    let (gf, ga) = build("union", &ids)?;
    let (wf, wa) = build("wkb", &ids)?;
    let (bf, ba) = build("plain", &ids)?;
    let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false), gf, wf, bf]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![Arc::new(Int64Array::from(vec![1i64, 2])), ga, wa, ba],
    )?;
    ctx.register_table("t", Arc::new(MemTable::try_new(schema, vec![vec![batch]])?))?;
    Ok(ctx)
}

/// The construct matrix. `{x}` is the source expression (a column or a UDF call over `id`),
/// `{c}` a constant source (UDF over a literal, constant-folded), `{ty}` the SQL/arrow type.
fn constructs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("control: projection", "SELECT {x} AS x FROM t"),
        ("CASE (two branches)", "SELECT CASE WHEN id = 1 THEN {x} ELSE {x} END AS x FROM t"),
        ("CASE (ELSE NULL)", "SELECT CASE WHEN id = 1 THEN {x} END AS x FROM t"),
        ("COALESCE(x, x)", "SELECT COALESCE({x}, {x}) AS x FROM t"),
        ("COALESCE(x, NULL)", "SELECT COALESCE({x}, NULL) AS x FROM t"),
        ("make_array + unnest", "SELECT unnest(make_array({x}, {x})) AS x FROM t"),
        ("make_array (element field)", "SELECT make_array({x}, {x}) AS x FROM t"),
        ("array_agg + unnest", "SELECT unnest(array_agg({x})) AS x FROM t"),
        ("array_agg (element field)", "SELECT array_agg({x}) AS x FROM t"),
        ("UNION ALL", "SELECT {x} AS x FROM t UNION ALL SELECT {x} AS x FROM t"),
        ("VALUES (constants)", "SELECT column1 AS x FROM (VALUES ({c1}), ({c2}))"),
        ("CAST to own storage type", "SELECT CAST({x} AS {ty}) AS x FROM t"),
        ("arrow_cast to own storage type", "SELECT arrow_cast({x}, '{aty}') AS x FROM t"),
        ("CAST to VARCHAR (should drop)", "SELECT CAST({x} AS VARCHAR) AS x FROM t"),
    ]
}

fn element_md(f: &Field) -> HashMap<String, String> {
    match f.data_type() {
        DataType::List(inner) | DataType::LargeList(inner) | DataType::ListView(inner) => {
            inner.metadata().clone()
        }
        _ => f.metadata().clone(),
    }
}

pub async fn run(version: &str) -> Result<()> {
    let ctx = context()?;
    let expected = |kind: &str| -> String {
        let (f, _) = build(kind, &[]).unwrap();
        describe(f.metadata())
    };
    println!("version\tkind\tsource\tconstruct\tstatus\tout_field\tudf_sees\tkeeps\tdetail");
    for kind in ["union", "wkb", "plain"] {
        let (storage, _) = build(kind, &[])?;
        let storage = storage.data_type().clone();
        let ty = if kind != "union" { "BYTEA".to_string() } else { format!("{storage}") };
        for source in ["column", "udf"] {
            if kind == "plain" && source == "udf" {
                continue;
            }
            let x = match (kind, source) {
                ("plain", _) => "b".to_string(),
                ("union", "column") => "g".to_string(),
                ("wkb", "column") => "w".to_string(),
                (k, _) => format!("mk_{k}(id)"),
            };
            for (name, tpl) in constructs() {
                if name.starts_with("VALUES") && source == "column" {
                    continue;
                }
                let sql = tpl
                    .replace("{x}", &x)
                    .replace("{c1}", &format!("mk_{kind}(1)"))
                    .replace("{c2}", &format!("mk_{kind}(2)"))
                    .replace("{ty}", &ty)
                    .replace("{aty}", &format!("{storage}"));
                let res = async {
                    // Planned output field (logical plan schema), then execution.
                    let df = ctx.sql(&sql).await?;
                    let planned = df.schema().inner().field(0).clone();
                    let exec = match df.collect().await {
                        Ok(batches) => {
                            let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
                            let same = batches
                                .first()
                                .map(|b| b.schema().field(0).metadata() == planned.metadata())
                                .unwrap_or(true);
                            format!(
                                "ok ({rows} rows, {}{})",
                                short(planned.data_type()),
                                if same { "" } else { ", batch metadata differs from plan" }
                            )
                        }
                        Err(e) => format!("plan ok, exec ERROR: {}", first_line(&e.to_string())),
                    };
                    let udf = match ctx.sql(&format!("SELECT meta_of(x) FROM ({sql}) s LIMIT 1")).await {
                        Err(e) => format!("ERROR {}", first_line(&e.to_string())),
                        Ok(df) => df
                            .collect()
                            .await
                            .map(|b| {
                                b.first()
                                    .and_then(|b| {
                                        b.column(0)
                                            .as_any()
                                            .downcast_ref::<StringArray>()
                                            .map(|s| s.value(0).to_string())
                                    })
                                    .unwrap_or_default()
                            })
                            .unwrap_or_else(|e| format!("ERROR {}", first_line(&e.to_string()))),
                    };
                    Ok::<_, DataFusionError>((planned, exec, udf))
                }
                .await;
                match res {
                    Ok((field, status, udf)) => {
                        let out = describe(&element_md(&field));
                        let exp = expected(kind);
                        let keeps = if kind == "plain" {
                            if status.starts_with("ok") { "n/a (control, works)" } else { "n/a (control, exec error)" }
                        } else if name.contains("should drop") {
                            if out == "-" { "dropped (correct)" } else { "KEPT (wrong)" }
                        } else if !status.starts_with("ok") {
                            "NO (exec error)"
                        } else if out == exp && udf.ends_with(&exp) {
                            "yes"
                        } else if out == exp {
                            "output only"
                        } else if udf.ends_with(&exp) {
                            "udf only"
                        } else {
                            "NO"
                        };
                        println!("{version}\t{kind}\t{source}\t{name}\t{status}\t{out}\t{udf}\t{keeps}\t{sql}");
                    }
                    Err(e) => println!(
                        "{version}\t{kind}\t{source}\t{name}\tERROR\t\t\t{}\t{sql} => {}",
                        if kind == "plain" { "n/a (control, error)" } else { "NO (error)" },
                        first_line(&e.to_string())
                    ),
                }
            }
        }
    }
    Ok(())
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(300).collect()
}

fn short(t: &DataType) -> String {
    match t {
        DataType::Union(..) => "Union".to_string(),
        DataType::List(f) => format!("List({})", short(f.data_type())),
        DataType::LargeList(f) => format!("LargeList({})", short(f.data_type())),
        other => format!("{other}"),
    }
}
