//! H2c: the same reproductions against geoarrow-array 0.9 (latest release).
#[path = "../../rust/src/shared/roundtrip.rs"]
mod roundtrip;

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    println!("## geoarrow-array 0.9.0 minimal reproductions\n");
    roundtrip::minimal_repros();
    if let Some(path) = std::env::args().nth(1) {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let (mut n, mut changed, mut wkb_changed) = (0, vec![], 0);
        for lit in v["lits"].as_array().unwrap() {
            let body = roundtrip::strip_srid(lit["text"].as_str().unwrap());
            if let Ok(Ok(g)) = std::panic::catch_unwind(|| body.parse::<wkt::Wkt<f64>>()) {
                n += 1;
                if !matches!(roundtrip::native(&g), roundtrip::Rt::Same) {
                    changed.push(format!("{}:{}", lit["file"].as_str().unwrap(), lit["line"]));
                }
                if !matches!(roundtrip::wkb_rt(&g), roundtrip::Rt::Same) {
                    wkb_changed += 1;
                }
            }
        }
        println!("\nDoc literals parsed: {n}; native round trip changed: {} {:?}; WKB changed: {wkb_changed}", changed.len(), changed);
    }
}
