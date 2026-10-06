//! E7 H2f: round-trip the E4 doc-test geometry literals through `geoarrow.geometry` builders:
//! geoarrow-array's `GeometryBuilder` (released, or patched) and the local builder
//! (`shared/union_builder.rs`). Each value goes builder → `GeometryArray` → Arrow `UnionArray`
//! (`into_array_ref`) → `from_arrow_array` → `value(0)` → WKT, compared with the literal's WKT.
//!
//! Usage: <bin> <label> <path to experiments/e4-type-model/h2c_literals.json>
//! Prints one JSON line per parsed literal and, on stderr, a summary.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use geoarrow_array::array::from_arrow_array;
use geoarrow_array::builder::GeometryBuilder;
use geoarrow_array::cast::AsGeoArrowArray;
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::{CoordType, GeometryType};

#[path = "../shared/union_builder.rs"]
mod union_builder;
use union_builder::LocalGeometryBuilder;

fn to_wkt(g: &impl geo_traits::GeometryTrait<T = f64>) -> String {
    let mut s = String::new();
    match wkt::to_wkt::write_geometry(&mut s, g) {
        Ok(()) => s,
        Err(e) => format!("<wkt write error {e}>"),
    }
}

fn typ() -> GeometryType {
    GeometryType::new(Arc::new(Default::default())).with_coord_type(CoordType::Separated)
}

fn read_back(arr: Arc<dyn GeoArrowArray>) -> Result<String, String> {
    let field = arr.data_type().to_field("g", true);
    let arrow = arr.into_array_ref();
    let back = from_arrow_array(arrow.as_ref(), &field).map_err(|e| e.to_string())?;
    let g = back.as_geometry();
    let v = g.value(0).map_err(|e| e.to_string())?;
    Ok(to_wkt(&v))
}

fn roundtrip(builder: &str, g: &wkt::Wkt<f64>) -> String {
    let expected = to_wkt(g);
    let r = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
        let arr: Arc<dyn GeoArrowArray> = match builder {
            "geoarrow" => {
                let mut b = GeometryBuilder::new(typ());
                b.push_geometry(Some(g)).map_err(|e| e.to_string())?;
                Arc::new(b.finish())
            }
            _ => {
                let mut b = LocalGeometryBuilder::new(typ());
                b.push_geometry(Some(g)).map_err(|e| e.to_string())?;
                Arc::new(b.finish())
            }
        };
        read_back(arr)
    }));
    match r {
        Ok(Ok(s)) if s == expected => "same".to_string(),
        Ok(Ok(s)) => format!("CHANGED -> {s}"),
        Ok(Err(e)) => format!("ERROR {e}"),
        Err(p) => format!(
            "PANIC {}",
            p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
        ),
    }
}

fn strip_srid(s: &str) -> &str {
    if s.len() > 5 && s[..5].eq_ignore_ascii_case("SRID=") {
        if let Some(i) = s.find(';') {
            return &s[i + 1..];
        }
    }
    s
}

pub fn main_impl() {
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().collect();
    let label = &args[1];
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
    let mut counts = std::collections::BTreeMap::<(String, String), usize>::new();
    for lit in v["lits"].as_array().unwrap() {
        let text = lit["text"].as_str().unwrap();
        let Ok(Ok(g)) = catch_unwind(|| strip_srid(text).parse::<wkt::Wkt<f64>>()) else { continue };
        let mut o = serde_json::json!({
            "label": label, "file": lit["file"], "line": lit["line"], "kind": lit["kind"], "text": text,
        });
        for b in ["geoarrow", "local"] {
            let r = roundtrip(b, &g);
            let class = r.split(' ').next().unwrap().to_string();
            *counts.entry((b.to_string(), class)).or_default() += 1;
            o[b] = r.into();
        }
        println!("{o}");
    }
    for ((b, c), n) in counts {
        eprintln!("{label}\t{b}\t{c}\t{n}");
    }
}
