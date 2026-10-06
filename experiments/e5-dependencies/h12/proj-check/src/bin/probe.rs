//! Binary size probe: one ST_Transform-like call through the `proj` crate.
fn main() {
    let p = proj::Proj::new_known_crs("EPSG:4326", "EPSG:3857", None).unwrap();
    let v: f64 = std::env::args().count() as f64;
    println!("{:?}", p.convert((v, 42.0)).unwrap());
}
