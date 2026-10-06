//! Baseline for binary size: no PROJ.
fn main() {
    let v: f64 = std::env::args().count() as f64;
    println!("{}", v * 2.0);
}
