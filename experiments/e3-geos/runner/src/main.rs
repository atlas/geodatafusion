//! E3 runner: runs the corpus through GEOS operations, or canonicalises PostGIS output.
//!
//! `e3-geos-runner run <corpus.tsv> <out.tsv>`: one row per (id, op) with the raw result
//! (`G:<hex WKB>`, `T:<text>`, `E:<error>`) and the harness's canonical rendering.
//! `e3-geos-runner render <raw.tsv> <out.tsv>`: adds the canonical column to `id op raw` rows
//! produced elsewhere (PostGIS).
//! `e3-geos-runner version`: prints the linked GEOS version.

#[allow(dead_code)]
#[path = "../../../../rust/geodatafusion/tests/sqllogictests/render.rs"]
mod render;

use std::fs;
use std::io::Write;

use geos::{CoordDimensions, Geom, Geometry, GeometryTypes, WKBWriter};

fn to_raw(r: geos::GResult<Geometry>, has_z: bool) -> String {
    match r {
        Ok(g) => {
            let mut w = WKBWriter::new().expect("writer");
            w.set_output_dimension(if has_z { CoordDimensions::ThreeD } else { CoordDimensions::TwoD });
            match w.write_wkb(&g) {
                Ok(b) => format!("G:{}", hex(&b)),
                Err(e) => format!("E:wkb write: {e}"),
            }
        }
        Err(e) => format!("E:{}", e.to_string().replace(['\t', '\n'], " ")),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// The harness's rendering: canonical EWKT at 12 significant digits for geometries, text verbatim.
fn canonical(raw: &str) -> String {
    if let Some(h) = raw.strip_prefix("G:") {
        render::ewkb(&unhex(h), None)
    } else if let Some(t) = raw.strip_prefix("T:") {
        render::text(t)
    } else if raw.starts_with("E:") {
        "ERROR".to_string()
    } else {
        raw.to_string()
    }
}

fn is_lineal(g: &Geometry) -> bool {
    matches!(
        g.geometry_type(),
        Ok(GeometryTypes::LineString | GeometryTypes::MultiLineString | GeometryTypes::LinearRing)
    )
}

fn is_areal(g: &Geometry) -> bool {
    matches!(g.geometry_type(), Ok(GeometryTypes::Polygon | GeometryTypes::MultiPolygon))
}

fn run(corpus: &str, out: &str) {
    let text = fs::read_to_string(corpus).expect("read corpus");
    let mut o = fs::File::create(out).expect("create out");
    writeln!(o, "id\top\traw\tcanonical").unwrap();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let (id, param): (&str, f64) = (f[0], f[2].parse().unwrap());
        let a = match Geometry::new_from_wkb(&unhex(f[3])) {
            Ok(g) => g,
            Err(e) => {
                writeln!(o, "{id}\tparse\tE:{e}\tERROR").unwrap();
                continue;
            }
        };
        let z = a.has_z().unwrap_or(false);
        let mut results: Vec<(&str, String)> = Vec::new();
        if !f[4].is_empty() {
            let b = Geometry::new_from_wkb(&unhex(f[4])).expect("parse b");
            let zz = z || b.has_z().unwrap_or(false);
            results.push(("intersection", to_raw(a.intersection(&b), zz)));
            results.push(("union", to_raw(a.union(&b), zz)));
        } else {
            // PostGIS ST_Buffer default: quad_segs=8, round caps and joins.
            results.push(("buffer", to_raw(a.buffer(param, 8), z)));
            if is_areal(&a) {
                results.push(("buffer_neg", to_raw(a.buffer(-param, 8), z)));
            }
            results.push(("makevalid", to_raw(a.make_valid(), z)));
            results.push(("simplifypt", to_raw(a.topology_preserve_simplify(param), z)));
            results.push(("pointonsurface", to_raw(a.point_on_surface(), z)));
            results.push(("convexhull", to_raw(a.convex_hull(), z)));
            if is_lineal(&a) {
                results.push(("linemerge", to_raw(a.line_merge(), z)));
            }
            let reason = match a.is_valid_reason() {
                Ok(s) => format!("T:{}", s.replace(['\t', '\n'], " ")),
                Err(e) => format!("E:{e}"),
            };
            results.push(("isvalidreason", reason));
        }
        for (op, raw) in results {
            let c = canonical(&raw);
            writeln!(o, "{id}\t{op}\t{raw}\t{c}").unwrap();
        }
    }
}

fn render_file(input: &str, out: &str) {
    let text = fs::read_to_string(input).expect("read input");
    let mut o = fs::File::create(out).expect("create out");
    writeln!(o, "id\top\traw\tcanonical").unwrap();
    for line in text.lines() {
        let mut f = line.splitn(3, '\t');
        let (id, op, raw) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap_or(""));
        writeln!(o, "{id}\t{op}\t{raw}\t{}", canonical(raw)).unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => run(&args[2], &args[3]),
        Some("render") => render_file(&args[2], &args[3]),
        Some("version") => println!("{}", geos::version().unwrap()),
        _ => eprintln!("usage: run <corpus> <out> | render <raw> <out> | version"),
    }
}
