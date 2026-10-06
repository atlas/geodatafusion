//! E1 performance experiment (plans/hypotheses.md, H1/H2/H6).
//!
//! Usage:
//!   e1 run   <dataset> <enc> <rows> <reps> <query>...   interleaved wall-clock runs (TSV)
//!   e1 cg    <e2e|kernel> <dataset> <enc> <rows> <query> one counted run under cachegrind
//!   e1 sizes <dataset> <enc> <rows>                      native vs WKB output sizes (H2)
//!   e1 list                                              print the query ids
//!
//! dataset: points | poly10 | poly100 | poly1000; enc: sep | int | wkb.

mod cg;
mod column;
mod data;
mod kernels;
mod udf;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, Int32Type};
use arrow_array::{Array, RecordBatch};
use arrow_data::ArrayData;
use arrow_schema::DataType;
use datafusion::datasource::MemTable;
use datafusion::prelude::{SessionConfig, SessionContext};

use crate::data::{Dataset, Encoding};
use crate::udf::{BenchUdf, KERNEL_NS, Op, Out, Style, WkbConst};

const BATCH_SIZE: usize = 8192;

fn register_udfs(ctx: &SessionContext) {
    // geodatafusion's own UDFs: st_area/st_centroid/st_simplify/st_intersects call
    // geoarrow-expr-geo (H6 baseline), st_x/st_npoints/st_isempty are today's typed accessors.
    geodatafusion::register(ctx);
    ctx.register_udf(WkbConst::udf());
    ctx.register_udf(BenchUdf::udf("e_intersects", Op::ExprGeoIntersects, Style::Typed, Out::Native));
    let ops = [
        ("x", Op::X),
        ("npoints", Op::NPoints),
        ("isempty", Op::IsEmpty),
        ("area", Op::Area),
        ("centroid", Op::Centroid),
        ("intersects", Op::Intersects),
        ("buffer", Op::Buffer),
        ("simplify", Op::Simplify),
        ("translate", Op::Translate),
        ("astext", Op::AsText),
        ("intersectsrelate", Op::IntersectsRelate),
    ];
    for (s, style) in [("t", Style::Typed), ("u", Style::Unified), ("v", Style::UnifiedFast)] {
        for (name, op) in ops {
            for (suffix, out) in [("", Out::Native), ("_wkb", Out::Wkb), ("_same", Out::Same)] {
                let full = format!("{s}_{name}{suffix}");
                ctx.register_udf(BenchUdf::udf(&full, op, style, out));
            }
        }
    }
}

fn constant_wkt() -> String {
    data::polygon_wkt(&data::constant_polygon())
}

/// Query id → SQL.
fn query_sql(id: &str) -> String {
    let q = format!("e1_wkb('{}')", constant_wkt());
    let parts: Vec<&str> = id.split('_').collect();
    match parts.as_slice() {
        // H1 / H6 single functions: <style>_<fn>[_aa|_same], style t (typed), u (unified),
        // g (geodatafusion today = geoarrow-expr-geo for the geo functions).
        [s, f, rest @ ..] if ["t", "u", "v", "g", "e"].contains(s) => {
            let name = if *s == "g" {
                format!("st_{f}")
            } else if rest.contains(&"same") {
                format!("{s}_{f}_same")
            } else {
                format!("{s}_{f}")
            };
            let array_array = rest.contains(&"aa");
            let args = match *f {
                "intersects" | "intersectsrelate" if array_array => "geom, q".to_string(),
                "intersects" | "intersectsrelate" => format!("geom, {q}"),
                "buffer" => "geom, 0.1".to_string(),
                "simplify" => "geom, 0.1".to_string(),
                "translate" => "geom, 1.0, 2.0".to_string(),
                _ => "geom".to_string(),
            };
            format!("SELECT {name}({args}) AS r FROM t")
        }
        // H2 pipelines: h2_<p1..p4>_<style>_<n|w>.
        ["h2", p, s, o] => {
            let o = if *o == "w" { "_wkb" } else { "" };
            let expr = match *p {
                "p1" => format!("{s}_x({s}_centroid{o}(geom))"),
                "p2" => format!("{s}_area({s}_simplify{o}(geom, 0.1))"),
                "p3" => format!("{s}_astext({s}_translate{o}(geom, 1.0, 2.0))"),
                "p4" => format!("{s}_intersects({s}_buffer{o}(geom, 0.1), {q})"),
                _ => panic!("unknown pipeline {p}"),
            };
            format!("SELECT {expr} AS r FROM t")
        }
        _ => panic!("unknown query id {id}"),
    }
}

fn needs_q_column(queries: &[String]) -> bool {
    queries.iter().any(|q| q.ends_with("_aa"))
}

fn make_ctx(dataset: Dataset, enc: Encoding, rows: usize, with_q: bool) -> SessionContext {
    let config = SessionConfig::new()
        .with_target_partitions(1)
        .with_batch_size(BATCH_SIZE);
    let ctx = SessionContext::new_with_config(config);
    register_udfs(&ctx);
    let (schema, batches) = data::make_batches(dataset, enc, rows, BATCH_SIZE, with_q);
    let table = MemTable::try_new(schema, vec![batches]).unwrap();
    ctx.register_table("t", Arc::new(table)).unwrap();
    ctx
}

/// A cheap fingerprint of the result, to check that variants agree.
fn checksum(batches: &[RecordBatch]) -> String {
    let mut non_null = 0usize;
    let mut sum = 0f64;
    for b in batches {
        let c = b.column(0);
        non_null += c.len() - c.null_count();
        match c.data_type() {
            DataType::Float64 => {
                sum += c.as_primitive::<Float64Type>().iter().flatten().sum::<f64>()
            }
            DataType::Int32 => {
                sum += c.as_primitive::<Int32Type>().iter().flatten().map(|v| v as f64).sum::<f64>()
            }
            DataType::Boolean => sum += c.as_boolean().true_count() as f64,
            DataType::Utf8 => {
                sum += c.as_string::<i32>().iter().flatten().map(|s| s.len() as f64).sum::<f64>()
            }
            _ => {}
        }
    }
    format!("{non_null}:{sum:.6e}")
}

async fn run_query(ctx: &SessionContext, sql: &str, region: u8) -> (f64, f64, String) {
    KERNEL_NS.store(0, Ordering::Relaxed);
    cg::start(region);
    let t0 = Instant::now();
    let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
    let wall = t0.elapsed().as_secs_f64() * 1e3;
    cg::stop(region);
    let kernel = KERNEL_NS.load(Ordering::Relaxed) as f64 / 1e6;
    let sum = checksum(&batches);
    drop(batches);
    (wall, kernel, sum)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().build().unwrap()
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
            let ctx = make_ctx(dataset, enc, rows, needs_q_column(&queries));
            let rt = runtime();
            // Rep 0 is a warm-up and isn't reported. The order rotates every rep, so no variant
            // always runs first or after the same neighbour.
            for rep in 0..=reps {
                for k in 0..queries.len() {
                    let id = &queries[(k + rep) % queries.len()];
                    let sql = query_sql(id);
                    let (wall, kernel, sum) = rt.block_on(run_query(&ctx, &sql, 0));
                    if rep > 0 {
                        println!(
                            "{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{}",
                            args[2], args[3], rows, id, rep, wall, kernel, sum
                        );
                    }
                }
            }
        }
        Some("cg") => {
            let region: u8 = match args[2].as_str() {
                "e2e" => 1,
                "kernel" => 2,
                r => panic!("region: e2e | kernel, got {r}"),
            };
            let dataset = Dataset::parse(&args[3]);
            let enc = Encoding::parse(&args[4]);
            let rows: usize = args[5].parse().unwrap();
            let id = args[6].clone();
            let ctx = make_ctx(dataset, enc, rows, id.ends_with("_aa"));
            let rt = runtime();
            let sql = query_sql(&id);
            // One uncounted warm-up, then one counted run.
            let (_, _, sum0) = rt.block_on(run_query(&ctx, &sql, 0));
            cg::REGION.store(region, Ordering::Relaxed);
            let (_, _, sum) = rt.block_on(run_query(&ctx, &sql, region));
            cg::REGION.store(0, Ordering::Relaxed);
            assert_eq!(sum0, sum);
            eprintln!("checksum {sum}");
        }
        Some("sizes") => {
            let dataset = Dataset::parse(&args[2]);
            let enc = Encoding::parse(&args[3]);
            let rows: usize = args[4].parse().unwrap();
            let ctx = make_ctx(dataset, enc, rows, false);
            let rt = runtime();
            for f in ["centroid", "simplify", "translate", "buffer"] {
                for o in ["", "_wkb"] {
                    let args = match f {
                        "simplify" | "buffer" => "geom, 0.1",
                        "translate" => "geom, 1.0, 2.0",
                        _ => "geom",
                    };
                    let sql = format!("SELECT u_{f}{o}({args}) AS r FROM t");
                    let batches =
                        rt.block_on(async { ctx.sql(&sql).await.unwrap().collect().await.unwrap() });
                    let bytes: usize = batches.iter().map(|b| data_bytes(&b.column(0).to_data())).sum();
                    let alloc: usize =
                        batches.iter().map(|b| b.column(0).get_array_memory_size()).sum();
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        args_dataset(&dataset),
                        enc_name(enc),
                        rows,
                        f,
                        if o.is_empty() { "native" } else { "wkb" },
                        bytes,
                        alloc
                    );
                }
            }
        }
        Some("list") | None => {
            for id in ["t_x", "u_x", "h2_p1_u_n", "u_intersects_aa", "u_simplify_same"] {
                println!("{id}: {}", query_sql(id));
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

fn args_dataset(d: &Dataset) -> String {
    match d {
        Dataset::Points => "points".into(),
        Dataset::Polygons(n) => format!("poly{n}"),
    }
}

fn enc_name(e: Encoding) -> &'static str {
    match e {
        Encoding::Separated => "sep",
        Encoding::Interleaved => "int",
        Encoding::Wkb => "wkb",
    }
}
