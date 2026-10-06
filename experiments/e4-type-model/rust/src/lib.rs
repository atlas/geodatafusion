//! Shared helpers for the E4 experiments.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{Field, Schema};
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::{PointBuilder, WkbBuilder};
use geoarrow_schema::{CoordType, Crs, Dimension, Metadata, PointType, WkbType};

fn wkt(s: &str) -> wkt::Wkt<f64> {
    s.parse().unwrap()
}

fn wkb_col(rows: &[&str], crs: Option<&str>) -> (ArrayRef, Field, String) {
    let meta = match crs {
        Some(c) => Metadata::new(Crs::from_authority_code(c.to_string()), None),
        None => Metadata::default(),
    };
    let mut b = WkbBuilder::<i32>::new(WkbType::new(Arc::new(meta)));
    for r in rows {
        b.push_geometry(Some(&wkt(r))).unwrap();
    }
    let arr = b.finish();
    let f = arr.data_type().to_field("x", true);
    (arr.to_array_ref(), f, String::new())
}

fn point_col(rows: &[&str], ct: CoordType, dim: Dimension) -> (ArrayRef, Field, String) {
    let typ = PointType::new(dim, Default::default()).with_coord_type(ct);
    let mut b = PointBuilder::new(typ);
    for r in rows {
        b.push_geometry(Some(&wkt(r))).unwrap();
    }
    let arr = b.finish();
    let f = arr.data_type().to_field("x", true);
    (arr.to_array_ref(), f, String::new())
}

/// Table `t` (2 rows):
/// - id: 1, 2
/// - wkb: geoarrow.wkb, no CRS: POINT(1 2), LINESTRING(0 0,2 4)   (centroid of both = POINT(1 2))
/// - wkb4326: geoarrow.wkb, EPSG:4326: same values
/// - pt_sep: geoarrow.point XY separated: POINT(1 2), POINT(3 4)
/// - pt_il: geoarrow.point XY interleaved: POINT(1 2), POINT(3 4)
/// - pt_z: geoarrow.point XYZ separated: POINT Z(1 2 3), POINT Z(3 4 5)
/// - wkt: geoarrow.wkt (Utf8): POINT(1 2), LINESTRING(0 0,2 4)
pub fn test_table() -> RecordBatch {
    let g = ["POINT(1 2)", "LINESTRING(0 0,2 4)"];
    let p = ["POINT(1 2)", "POINT(3 4)"];
    let pz = ["POINT Z(1 2 3)", "POINT Z(3 4 5)"];
    let mut cols: Vec<(&str, ArrayRef, Field)> = vec![];
    let id: ArrayRef = Arc::new(Int64Array::from(vec![1, 2]));
    cols.push(("id", id, Field::new("id", arrow_schema::DataType::Int64, false)));
    let (a, f, _) = wkb_col(&g, None);
    cols.push(("wkb", a, f));
    let (a, f, _) = wkb_col(&g, Some("EPSG:4326"));
    cols.push(("wkb4326", a, f));
    let (a, f, _) = point_col(&p, CoordType::Separated, Dimension::XY);
    cols.push(("pt_sep", a, f));
    let (a, f, _) = point_col(&p, CoordType::Interleaved, Dimension::XY);
    cols.push(("pt_il", a, f));
    let (a, f, _) = point_col(&pz, CoordType::Separated, Dimension::XYZ);
    cols.push(("pt_z", a, f));
    let wkt_arr: ArrayRef = Arc::new(StringArray::from(g.to_vec()));
    let wkt_field = Field::new("wkt", arrow_schema::DataType::Utf8, true).with_metadata(
        HashMap::from([("ARROW:extension:name".to_string(), "geoarrow.wkt".to_string())]),
    );
    cols.push(("wkt", wkt_arr, wkt_field));

    let fields: Vec<Field> = cols
        .iter()
        .map(|(n, _, f)| f.clone().with_name(*n))
        .collect();
    let arrays: Vec<ArrayRef> = cols.into_iter().map(|(_, a, _)| a).collect();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
}

pub fn context() -> SessionContext {
    let ctx = SessionContext::new();
    geodatafusion::register(&ctx);
    let batch = test_table();
    let table = MemTable::try_new(batch.schema(), vec![vec![batch]]).unwrap();
    ctx.register_table("t", Arc::new(table)).unwrap();
    ctx
}

/// Outcome of running one query.
pub struct Outcome {
    pub ok: bool,
    pub stage: &'static str,
    pub detail: String,
    /// Output fields (type + extension name + extension metadata).
    pub fields: Vec<String>,
}

pub fn describe_field(f: &Field) -> String {
    let ext = f
        .metadata()
        .get("ARROW:extension:name")
        .cloned()
        .unwrap_or_else(|| "-".into());
    let meta = f
        .metadata()
        .get("ARROW:extension:metadata")
        .cloned()
        .unwrap_or_default();
    let dt = short_type(f.data_type());
    if meta.is_empty() || meta == "{}" {
        format!("{dt} [{ext}]")
    } else {
        format!("{dt} [{ext} {meta}]")
    }
}

pub fn short_type(dt: &arrow_schema::DataType) -> String {
    use arrow_schema::DataType::*;
    match dt {
        Struct(fs) => format!(
            "Struct<{}>",
            fs.iter().map(|f| f.name().as_str()).collect::<Vec<_>>().join(",")
        ),
        FixedSizeList(_, n) => format!("FixedSizeList[{n}]"),
        Union(..) => "Union(geometry)".into(),
        List(f) => format!("List<{}>", describe_field(f)),
        other => format!("{other}"),
    }
}

pub fn one_line(s: &str) -> String {
    let s = s.replace('\n', " ").replace('|', "\\|");
    if s.len() > 220 { format!("{}...", &s[..220]) } else { s }
}

pub async fn run(ctx: &SessionContext, sql: &str) -> Outcome {
    let df = match ctx.sql(sql).await {
        Ok(df) => df,
        Err(e) => {
            return Outcome { ok: false, stage: "plan", detail: one_line(&e.to_string()), fields: vec![] };
        }
    };
    let fields: Vec<String> = df.schema().fields().iter().map(|f| describe_field(f)).collect();
    match df.collect().await {
        Ok(batches) => {
            let mut rows = vec![];
            for b in &batches {
                for r in 0..b.num_rows() {
                    let mut cells = vec![];
                    for c in b.columns() {
                        let s = if c.is_null(r) {
                            "NULL".to_string()
                        } else {
                            arrow_cast::display::array_value_to_string(c, r)
                                .unwrap_or_else(|e| format!("<{e}>"))
                        };
                        cells.push(s);
                    }
                    rows.push(cells.join(", "));
                }
            }
            Outcome { ok: true, stage: "ok", detail: one_line(&rows.join("; ")), fields }
        }
        Err(e) => Outcome { ok: false, stage: "exec", detail: one_line(&e.to_string()), fields },
    }
}
