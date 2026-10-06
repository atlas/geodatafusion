//! H2c: round-trip every geometry literal / expected value of the postgis_docs records through
//! geoarrow-array 0.8's GeometryBuilder and WkbBuilder; then the same bugs end to end through
//! geodatafusion. Writes one JSON line per literal to stdout after a marker, for h2c_count.py.

#[path = "../shared/roundtrip.rs"]
mod roundtrip;

use roundtrip::*;

#[tokio::main]
async fn main() {
    if std::env::var("E4_SHOW_PANICS").is_err() { std::panic::set_hook(Box::new(|_| {})); }
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("repro") {
        println!("## geoarrow-array 0.8.0 minimal reproductions\n");
        minimal_repros();
        println!("\n## End to end through geodatafusion (DataFusion 54)\n");
        let ctx = e4_type_model::context();
        let hex: String = mixed_dim_gc_wkb().iter().map(|b| format!("{b:02x}")).collect();
        for sql in [
            "SELECT ST_AsText(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))'))".to_string(),
            "SELECT ST_GeometryType(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 2))'))".to_string(),
            "SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('GEOMETRYCOLLECTION(LINESTRING(0 0,1 1))'))))".to_string(),
            "SELECT ST_AsText(ST_GeomFromText('MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))'))".to_string(),
            format!("SELECT ST_AsText(ST_GeomFromWKB(decode('{hex}', 'hex')))"),
            format!("SELECT ST_AsText(decode('{hex}', 'hex'))"),
        ] {
            let c2 = ctx.clone();
            let q = sql.clone();
            let res = match tokio::spawn(async move { e4_type_model::run(&c2, &q).await }).await {
                Ok(o) if o.ok => format!("ok → {}", o.detail),
                Ok(o) => format!("FAIL ({}): {}", o.stage, o.detail),
                Err(e) => format!("PANIC (task): {}", { let p = e.into_panic(); p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default() }),
            };
            println!("- `{sql}` → {res}");
        }
        return;
    }
    let path = args.get(1).expect("h2c_literals.json");
    let data = std::fs::read_to_string(path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&data).unwrap();
    for lit in v["lits"].as_array().unwrap() {
        let text = lit["text"].as_str().unwrap();
        let body = strip_srid(text);
        let parsed = std::panic::catch_unwind(|| body.parse::<wkt::Wkt<f64>>());
        let (native, wkb, parse) = match parsed {
            Ok(Ok(g)) => (native(&g).label(), wkb_rt(&g).label(), "ok".to_string()),
            Ok(Err(e)) => ("-".into(), "-".into(), format!("parse error: {e}")),
            Err(_) => ("-".into(), "-".into(), "parse PANIC".to_string()),
        };
        let mut o = lit.clone();
        o["native"] = native.into();
        o["wkb"] = wkb.into();
        o["parse"] = parse.into();
        println!("{o}");
    }
}
