//! E7 H2d: union-only (`geoarrow.geometry`) vs WKB geometry outputs in multi-step pipelines,
//! typed consumers. Built on E1's harness (inputs, measurement, cachegrind regions).
//!
//! Usage:
//!   e7 run    <dataset> <enc> <rows> <reps> <query>...   interleaved wall-clock runs (TSV)
//!   e7 cg     <dataset> <enc> <rows> <query>             one counted end-to-end run (cachegrind)
//!   e7 verify <dataset> <enc> <rows>                     n == l and w == f outputs, per producer
//!   e7 sizes  <dataset> <enc> <rows>                     output buffer bytes per producer
//!   e7 list
//!
//! dataset: points | poly10 | poly100 | poly1000; enc: sep | wkb.
//! query: <p1..p5>_<n|l|w|f>.

mod cg;
mod data;
mod kernels;
mod output;
mod udf;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use arrow_array::{Array, RecordBatch};
use arrow_data::ArrayData;
use arrow_schema::DataType;
use datafusion::datasource::MemTable;
use datafusion::prelude::{SessionConfig, SessionContext};

use crate::data::{Dataset, Encoding};

const BATCH_SIZE: usize = 8192;

fn constant_wkt() -> String {
    data::polygon_wkt(&data::constant_polygon())
}

fn pipeline_sql(p: &str, o: &str) -> String {
    let q = format!("e1_wkb('{}')", constant_wkt());
    let expr = match p {
        "p1" => format!("x(centroid_{o}(geom))"),
        "p2" => format!("area(simplify_{o}(geom, 0.1))"),
        "p3" => format!("astext(translate_{o}(geom, 1.0, 2.0))"),
        "p4" => format!("intersects(buffer_{o}(geom, 0.1), {q})"),
        "p5" => format!("x(centroid_{o}(simplify_{o}(translate_{o}(geom, 1.0, 2.0), 0.1)))"),
        _ => panic!("unknown pipeline {p}"),
    };
    format!("SELECT {expr} AS r FROM t")
}

fn query_sql(id: &str) -> String {
    let (p, o) = id.split_once('_').expect("query id <p>_<o>");
    pipeline_sql(p, o)
}

fn make_ctx(dataset: Dataset, enc: Encoding, rows: usize) -> SessionContext {
    let config = SessionConfig::new().with_target_partitions(1).with_batch_size(BATCH_SIZE);
    let ctx = SessionContext::new_with_config(config);
    udf::register(&ctx);
    let (schema, batches) = data::make_batches(dataset, enc, rows, BATCH_SIZE, false);
    let table = MemTable::try_new(schema, vec![batches]).unwrap();
    ctx.register_table("t", Arc::new(table)).unwrap();
    ctx
}

/// A fingerprint of the result, to check that variants agree.
fn checksum(batches: &[RecordBatch]) -> String {
    let mut non_null = 0usize;
    let mut sum = 0f64;
    for b in batches {
        let c = b.column(0);
        non_null += c.len() - c.null_count();
        match c.data_type() {
            DataType::Float64 => sum += c.as_primitive::<Float64Type>().iter().flatten().sum::<f64>(),
            DataType::Boolean => sum += c.as_boolean().true_count() as f64,
            DataType::Utf8 => {
                sum += c.as_string::<i32>().iter().flatten().map(|s| s.len() as f64).sum::<f64>()
            }
            _ => {}
        }
    }
    format!("{non_null}:{sum:.9e}")
}

async fn run_query(ctx: &SessionContext, sql: &str, region: u8) -> (f64, String) {
    cg::start(region);
    let t0 = Instant::now();
    let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
    let wall = t0.elapsed().as_secs_f64() * 1e3;
    cg::stop(region);
    let sum = checksum(&batches);
    drop(batches);
    (wall, sum)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().build().unwrap()
}

fn collect(ctx: &SessionContext, rt: &tokio::runtime::Runtime, sql: &str) -> Vec<RecordBatch> {
    rt.block_on(async { ctx.sql(sql).await.unwrap().collect().await.unwrap() })
}

fn producer_sql(f: &str, o: &str) -> String {
    let args = match f {
        "simplify" | "buffer" => "geom, 0.1",
        "translate" => "geom, 1.0, 2.0",
        _ => "geom",
    };
    format!("SELECT {f}_{o}({args}) AS r FROM t")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => {
            let dataset = Dataset::parse(&args[2]);
            let enc = Encoding::parse(&args[3]);
            let rows: usize = args[4].parse().unwrap();
            let reps: usize = args[5].parse().unwrap();
            let queries: Vec<String> = args[6..].to_vec();
            let ctx = make_ctx(dataset, enc, rows);
            let rt = runtime();
            // Rep 0 is a warm-up. The order rotates every rep.
            for rep in 0..=reps {
                for k in 0..queries.len() {
                    let id = &queries[(k + rep) % queries.len()];
                    let (wall, sum) = rt.block_on(run_query(&ctx, &query_sql(id), 0));
                    if rep > 0 {
                        println!("{}\t{}\t{}\t{}\t{}\t{:.3}\t{}", args[2], args[3], rows, id, rep, wall, sum);
                    }
                }
            }
        }
        Some("cg") => {
            let dataset = Dataset::parse(&args[2]);
            let enc = Encoding::parse(&args[3]);
            let rows: usize = args[4].parse().unwrap();
            let id = args[5].clone();
            let ctx = make_ctx(dataset, enc, rows);
            let rt = runtime();
            let sql = query_sql(&id);
            let (_, sum0) = rt.block_on(run_query(&ctx, &sql, 0));
            cg::REGION.store(1, Ordering::Relaxed);
            let (_, sum) = rt.block_on(run_query(&ctx, &sql, 1));
            cg::REGION.store(0, Ordering::Relaxed);
            assert_eq!(sum0, sum);
            eprintln!("checksum {sum}");
        }
        Some("verify") => {
            let dataset = Dataset::parse(&args[2]);
            let enc = Encoding::parse(&args[3]);
            let rows: usize = args[4].parse().unwrap();
            let ctx = make_ctx(dataset, enc, rows);
            let rt = runtime();
            for f in ["centroid", "simplify", "translate", "buffer"] {
                let get = |o: &str| collect(&ctx, &rt, &producer_sql(f, o));
                let (n, l, w, fa) = (get("n"), get("l"), get("w"), get("f"));
                // Union variants: same geometries row by row (compared as WKT).
                let wkt_rows = |bs: &[RecordBatch]| -> Vec<String> {
                    let mut v = Vec::new();
                    for b in bs {
                        let field = b.schema().field(0).clone();
                        let a = geoarrow_array::array::from_arrow_array(b.column(0).as_ref(), &field).unwrap();
                        let a = geoarrow_array::cast::to_wkb::<i32>(a.as_ref()).unwrap();
                        for g in geoarrow_array::GeoArrowArrayAccessor::iter(&a) {
                            let g = g.unwrap().unwrap();
                            let mut s = String::new();
                            wkt::to_wkt::write_geometry(&mut s, &g).unwrap();
                            v.push(s);
                        }
                    }
                    v
                };
                let (wn, wl, ww, wf) = (wkt_rows(&n), wkt_rows(&l), wkt_rows(&w), wkt_rows(&fa));
                let bytes_eq = w.iter().zip(&fa).all(|(a, b)| {
                    a.column(0).as_binary::<i32>().values() == b.column(0).as_binary::<i32>().values()
                });
                println!(
                    "{}\t{}\t{}\t{}\tn==l:{}\tn==w:{}\tw==f(wkt):{}\tw==f(bytes):{}",
                    args[2], args[3], rows, f, wn == wl, wn == ww, ww == wf, bytes_eq
                );
            }
        }
        Some("sizes") => {
            let dataset = Dataset::parse(&args[2]);
            let enc = Encoding::parse(&args[3]);
            let rows: usize = args[4].parse().unwrap();
            let ctx = make_ctx(dataset, enc, rows);
            let rt = runtime();
            for f in ["centroid", "simplify", "translate", "buffer"] {
                for o in ["n", "l", "w"] {
                    let batches = collect(&ctx, &rt, &producer_sql(f, o));
                    let bytes: usize = batches.iter().map(|b| data_bytes(&b.column(0).to_data())).sum();
                    println!("{}\t{}\t{}\t{}\t{}\t{}", args[2], args[3], rows, f, o, bytes);
                }
            }
        }
        Some("list") | None => {
            for p in ["p1", "p2", "p3", "p4", "p5"] {
                println!("{p}_n: {}", pipeline_sql(p, "n"));
            }
        }
        Some(other) => panic!("unknown mode {other}"),
    }
}

/// Bytes of buffer data (lengths, not capacities), recursively.
fn data_bytes(d: &ArrayData) -> usize {
    let own: usize = d.buffers().iter().map(|b| b.len()).sum();
    let nulls = d.nulls().map(|n| n.buffer().len()).unwrap_or(0);
    own + nulls + d.child_data().iter().map(data_bytes).sum::<usize>()
}
