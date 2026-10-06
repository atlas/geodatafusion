//! H9 (Rust side): which CRS do geoarrow-rs / geodatafusion's own readers and writers produce?
//!
//! 1. Read fixtures/geoparquet/nybb_wkb.parquet through geodatafusion-geoparquet and print the
//!    GeoArrow field metadata.
//! 2. Write GeoParquet with the geoparquet 0.8 encoder (default options) from a WKB column tagged
//!    with several CRS forms and print the `geo` metadata it writes.
//! 3. Read every GeoParquet file in a directory (produced by the Python tools) through
//!    geodatafusion-geoparquet and print the CRS it surfaces.
//! 4. Write a plain DataFusion Parquet file (COPY) of tagged columns, so the Python side can see
//!    what DataFusion itself exports.

use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use datafusion::execution::SessionStateBuilder;
use datafusion::prelude::SessionContext;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{Crs, Metadata, WkbType};
use geodatafusion_geoparquet::file_format::GeoParquetFormatFactory;
use geoparquet::writer::{GeoParquetRecordBatchEncoder, GeoParquetWriterOptionsBuilder};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

fn wkb_batch(crs: Crs) -> RecordBatch {
    let mut b = WkbBuilder::<i32>::new(WkbType::new(Arc::new(Metadata::new(crs, None))));
    let g: wkt::Wkt<f64> = "POINT(10 59)".parse().unwrap();
    b.push_geometry(Some(&g)).unwrap();
    let arr = b.finish();
    let field = arr.data_type().to_field("geometry", true);
    RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![arr.to_array_ref()]).unwrap()
}

async fn geoparquet_ctx() -> SessionContext {
    let state = SessionStateBuilder::new()
        .with_file_formats(vec![Arc::new(GeoParquetFormatFactory::default())])
        .build();
    let ctx = SessionContext::new_with_state(state).enable_url_table();
    geodatafusion::register(&ctx);
    ctx
}

async fn read_crs(ctx: &SessionContext, path: &str) -> String {
    match ctx.sql(&format!("SELECT * FROM '{path}' LIMIT 1")).await {
        Ok(df) => df
            .schema()
            .fields()
            .iter()
            .filter(|f| f.metadata().contains_key("ARROW:extension:name"))
            .map(|f| format!("{}: {}", f.name(), e4_type_model::describe_field(f)))
            .collect::<Vec<_>>()
            .join("; "),
        Err(e) => format!("ERROR {e}"),
    }
}

#[tokio::main]
async fn main() {
    let out = std::env::args().nth(1).expect("output dir");
    let pydir = std::env::args().nth(2);
    std::fs::create_dir_all(&out).unwrap();
    let ctx = geoparquet_ctx().await;

    println!("## 1. geodatafusion-geoparquet reading fixtures/geoparquet/nybb_wkb.parquet\n");
    let nybb = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../fixtures/geoparquet/nybb_wkb.parquet");
    let s = read_crs(&ctx, nybb).await;
    println!("{}\n", if s.len() > 600 { format!("{}... ({} chars)", &s[..600], s.len()) } else { s });

    println!("## 2. geoparquet 0.8 encoder (default options) writing a WKB column\n");
    let projjson: serde_json::Value = serde_json::json!({
        "$schema": "https://proj.org/schemas/v0.7/projjson.schema.json",
        "type": "GeographicCRS", "name": "WGS 84",
        "id": {"authority": "EPSG", "code": 4326}
    });
    let cases: Vec<(&str, Crs)> = vec![
        ("authority_code EPSG:4326", Crs::from_authority_code("EPSG:4326".into())),
        ("authority_code OGC:CRS84", Crs::from_authority_code("OGC:CRS84".into())),
        ("srid 4326", Crs::from_srid("4326".into())),
        ("projjson (abbreviated)", Crs::from_projjson(projjson)),
        ("unknown string EPSG:4326", Crs::from_unknown_crs_type("EPSG:4326".into())),
        ("none", Crs::default()),
    ];
    for (name, crs) in cases {
        let batch = wkb_batch(crs);
        let opts = GeoParquetWriterOptionsBuilder::default().build();
        let mut enc = GeoParquetRecordBatchEncoder::try_new(batch.schema().as_ref(), &opts).unwrap();
        let path = format!("{out}/geoarrow_rs_{}.parquet", name.replace([' ', ':', '(', ')'], "_"));
        let file = std::fs::File::create(&path).unwrap();
        let mut w = ArrowWriter::try_new(file, enc.target_schema(), Some(WriterProperties::default())).unwrap();
        let encoded = enc.encode_record_batch(&batch).unwrap();
        w.write(&encoded).unwrap();
        let kv = enc.into_keyvalue().unwrap();
        let geo = kv.value.clone().unwrap_or_default();
        w.append_key_value_metadata(kv);
        w.close().unwrap();
        let v: serde_json::Value = serde_json::from_str(&geo).unwrap();
        let crs = &v["columns"]["geometry"]["crs"];
        println!("- {name}: geo.columns.geometry.crs = {}  (key present: {})", crs, v["columns"]["geometry"].get("crs").is_some());
        println!("  read back via geodatafusion-geoparquet: {}", read_crs(&ctx, &path).await);
    }

    println!("\n## 3. geodatafusion-geoparquet reading files written by other tools\n");
    if let Some(dir) = pydir {
        let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for p in paths {
            if p.extension().map(|e| e == "parquet").unwrap_or(false) {
                let s = read_crs(&ctx, p.to_str().unwrap()).await;
                let s = if s.len() > 300 { format!("{}... ({} chars)", &s[..300], s.len()) } else { s };
                println!("- {}: {}", p.file_name().unwrap().to_string_lossy(), s);
            }
        }
    }

    println!("\n## 4. DataFusion COPY of tagged columns (no `geo` metadata)\n");
    let plain = SessionContext::new();
    geodatafusion::register(&plain);
    for (name, srid) in [("4326", 4326), ("3857", 3857)] {
        let path = format!("{out}/datafusion_copy_{name}.parquet");
        let sql = format!(
            "COPY (SELECT ST_AsBinary(ST_Point(10.0, 59.0, {srid})) AS geometry, ST_Point(10.0, 59.0, {srid}) AS pt) TO '{path}' STORED AS PARQUET"
        );
        let r = plain.sql(&sql).await.unwrap().collect().await;
        println!("- {path}: {:?}", r.map(|_| "ok"));
    }
}
