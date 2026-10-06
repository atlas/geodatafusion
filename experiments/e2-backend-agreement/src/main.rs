//! E2 backend agreement: compare `geo` 0.31 (raw and normalized) and GEOS 3.14.1 (bundled, raw
//! and with PostGIS's wrapper rules) with PostGIS results, under the parity harness's rendering.
//!
//! Inputs (from gen_corpus.py and pg_eval.py) in $E2_DATA (default target/experiments/e2):
//! unary.tsv, pairs.tsv, pg/<function>.tsv, pg/_valid_*.tsv.
//! Outputs: out/per_function/<function>.json (counts) and .jsonl (every disagreement).

mod norm;
mod render;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use geo::{
    Area, Centroid, ConvexHull, Distance, Euclidean, InteriorPoint, Length, MinimumRotatedRect,
    Relate, Simplify, SimplifyVw, Validation,
};
use geo_types::{Geometry, GeometryCollection};
use geos::Geom;
use serde::Serialize;

pub const BACKENDS: [&str; 4] = ["geo_raw", "geo_norm", "geos_raw", "geos_wrap"];
const UNARY: [&str; 9] = [
    "st_isvalid",
    "st_pointonsurface",
    "st_convexhull",
    "st_orientedenvelope",
    "st_simplify",
    "st_simplifyvw",
    "st_centroid",
    "st_area",
    "st_length",
];
const BINARY: [&str; 6] = [
    "st_distance",
    "st_contains",
    "st_intersects",
    "st_within",
    "st_touches",
    "st_relate",
];
pub const NA: &str = "N/A";

fn data_dir() -> PathBuf {
    std::env::var_os("E2_DATA").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/experiments/e2")
    })
}

// ------------------------------------------------------------------------------------------
// Rendering
// ------------------------------------------------------------------------------------------

pub fn render_geo(g: &Geometry) -> String {
    let mut buf = Vec::new();
    match wkb::writer::write_geometry(&mut buf, g, &Default::default()) {
        Ok(()) => render::ewkb(&buf, None),
        Err(e) => format!("ERROR: wkb write: {e}"),
    }
}

fn render_geos(g: &geos::Geometry) -> String {
    match g.to_wkb() {
        Ok(buf) => render::ewkb(&buf, None),
        Err(e) => format!("ERROR: {e}"),
    }
}

fn render_wkb(buf: &[u8]) -> String {
    render::ewkb(buf, None)
}

fn b(v: bool) -> String {
    v.to_string()
}

fn f(v: f64) -> String {
    render::float(v)
}

/// Render the PostGIS value under the harness rules.
fn render_pg(func: &str, raw: &str) -> String {
    if raw == "NULL" || raw.starts_with("ERROR") {
        return raw.to_string();
    }
    match func {
        "st_area" | "st_length" | "st_distance" => match raw.parse::<f64>() {
            Ok(v) => f(v),
            Err(_) => raw.to_string(),
        },
        "st_isvalid" | "st_contains" | "st_intersects" | "st_within" | "st_touches" | "st_relate" => {
            raw.to_string()
        }
        _ => render_wkb(&hex::decode(raw).expect("hex")),
    }
}

// ------------------------------------------------------------------------------------------
// Conversions
// ------------------------------------------------------------------------------------------

/// geoarrow-expr-geo's `geometry_to_geo`, which geodatafusion's `geo` functions use today:
/// an EMPTY point anywhere is an error.
fn geo_raw(w: &wkb::reader::Wkb) -> Result<Geometry, String> {
    use geo_traits::to_geo::*;
    use geo_traits::{GeometryCollectionTrait, GeometryTrait, GeometryType as T};
    fn conv(g: &impl GeometryTrait<T = f64>) -> Result<Geometry, String> {
        let err = || "geo crate does not support empty points.".to_string();
        Ok(match g.as_type() {
            T::Point(p) => Geometry::Point(p.try_to_point().ok_or_else(err)?),
            T::LineString(l) => Geometry::LineString(l.to_line_string()),
            T::Polygon(p) => Geometry::Polygon(p.to_polygon()),
            T::MultiPoint(m) => Geometry::MultiPoint(m.try_to_multi_point().ok_or_else(err)?),
            T::MultiLineString(m) => Geometry::MultiLineString(m.to_multi_line_string()),
            T::MultiPolygon(m) => Geometry::MultiPolygon(m.to_multi_polygon()),
            T::GeometryCollection(gc) => Geometry::GeometryCollection(GeometryCollection::new_from(
                gc.geometries().map(|c| conv(&c)).collect::<Result<_, _>>()?,
            )),
            _ => return Err("unsupported".into()),
        })
    }
    conv(w)
}

fn run<T>(func: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(func)).map_err(|e| {
        let msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        format!("PANIC: {}", msg.replace(['\n', '\t'], " "))
    })
}

fn flatten(r: Result<String, String>) -> String {
    r.unwrap_or_else(|e| e)
}

fn im_string(im: &geo::relate::IntersectionMatrix) -> String {
    use geo::coordinate_position::CoordPos::*;
    let mut s = String::new();
    for l in [Inside, OnBoundary, Outside] {
        for r in [Inside, OnBoundary, Outside] {
            s.push(match im.get(l, r) {
                geo::dimensions::Dimensions::Empty => 'F',
                geo::dimensions::Dimensions::ZeroDimensional => '0',
                geo::dimensions::Dimensions::OneDimensional => '1',
                geo::dimensions::Dimensions::TwoDimensional => '2',
            });
        }
    }
    s
}

/// Sum of GEOS lengths of the linear parts, recursively (PostGIS's ST_Length rule).
fn geos_linear_length(g: &geos::Geometry) -> Result<f64, String> {
    use geos::GeometryTypes::*;
    let t = g.geometry_type().map_err(|e| e.to_string())?;
    Ok(match t {
        LineString | LinearRing | MultiLineString => g.length().map_err(|e| e.to_string())?,
        GeometryCollection => {
            let mut s = 0.0;
            for i in 0..g.get_num_geometries().map_err(|e| e.to_string())? {
                let c = g.get_geometry_n(i).map_err(|e| e.to_string())?;
                s += geos_linear_length(&Geom::clone(&c).map_err(|e| e.to_string())?)?;
            }
            s
        }
        _ => 0.0,
    })
}

// ------------------------------------------------------------------------------------------
// Unary evaluation
// ------------------------------------------------------------------------------------------

struct UnaryInput<'a> {
    wkb: &'a [u8],
    tol: f64,
    vwtol: f64,
}

fn eval_unary(func: &str, inp: &UnaryInput) -> [String; 4] {
    let w = match wkb::reader::read_wkb(inp.wkb) {
        Ok(w) => w,
        Err(e) => {
            let s = format!("ERROR: wkb {e}");
            return [s.clone(), s.clone(), s.clone(), s];
        }
    };
    let input_rendered = render_wkb(inp.wkb);
    let raw = geo_raw(&w);
    let normed = norm::to_geo(&w);
    let geos_g = geos::Geometry::new_from_wkb(inp.wkb).map_err(|e| format!("ERROR: {e}"));
    let geos_empty = geos_g.as_ref().ok().map(|g| g.is_empty().unwrap_or(false));

    // geo raw: today's geoarrow-expr-geo kernels.
    let geo_raw_out = flatten(run(|| {
        let g = match &raw {
            Ok(g) => g,
            Err(e) => return format!("ERROR: {e}"),
        };
        match func {
            "st_isvalid" => b(g.is_valid()),
            "st_pointonsurface" => g.interior_point().map_or("NULL".into(), |p| render_geo(&p.into())),
            "st_convexhull" => render_geo(&g.convex_hull().into()),
            "st_orientedenvelope" => {
                g.minimum_rotated_rect().map_or("NULL".into(), |p| render_geo(&p.into()))
            }
            "st_simplify" => render_geo(&match g {
                Geometry::LineString(x) => x.simplify(inp.tol).into(),
                Geometry::Polygon(x) => x.simplify(inp.tol).into(),
                Geometry::MultiLineString(x) => x.simplify(inp.tol).into(),
                Geometry::MultiPolygon(x) => x.simplify(inp.tol).into(),
                other => other.clone(),
            }),
            "st_simplifyvw" => render_geo(&match g {
                Geometry::LineString(x) => x.simplify_vw(inp.vwtol).into(),
                Geometry::Polygon(x) => x.simplify_vw(inp.vwtol).into(),
                Geometry::MultiLineString(x) => x.simplify_vw(inp.vwtol).into(),
                Geometry::MultiPolygon(x) => x.simplify_vw(inp.vwtol).into(),
                other => other.clone(),
            }),
            "st_centroid" => g.centroid().map_or("NULL".into(), |p| render_geo(&p.into())),
            "st_area" => f(g.unsigned_area()),
            "st_length" => f(match g {
                Geometry::Line(l) => Euclidean.length(l),
                Geometry::LineString(l) => Euclidean.length(l),
                Geometry::MultiLineString(l) => Euclidean.length(l),
                _ => 0.0,
            }),
            _ => unreachable!(),
        }
    }));

    // geo normalized: the G2 plan's R6 rules on top of `geo`.
    let geo_norm_out = flatten(run(|| norm::unary(func, normed.as_ref(), &input_rendered, inp.tol, inp.vwtol)));

    // GEOS raw.
    let geos_raw_out = flatten(run(|| {
        let g = match &geos_g {
            Ok(g) => g,
            Err(e) => return e.clone(),
        };
        let r: Result<String, geos::Error> = (|| {
            Ok(match func {
                "st_isvalid" => b(g.is_valid()?),
                "st_pointonsurface" => render_geos(&g.point_on_surface()?),
                "st_convexhull" => render_geos(&g.convex_hull()?),
                "st_orientedenvelope" => render_geos(&g.minimum_rotated_rectangle()?),
                "st_simplify" => render_geos(&g.simplify(inp.tol)?),
                "st_simplifyvw" => NA.to_string(),
                "st_centroid" => render_geos(&g.get_centroid()?),
                "st_area" => f(g.area()?),
                "st_length" => f(g.length()?),
                _ => unreachable!(),
            })
        })();
        r.unwrap_or_else(|e| format!("ERROR: {e}"))
    }));

    // GEOS with PostGIS's wrapper rules (EMPTY shortcuts, conversion failures, ST_Length).
    let geos_wrap_out = flatten(run(|| {
        match (func, &geos_g, geos_empty) {
            // PostGIS reports geometries GEOS can't build (rings < 4 points) as invalid.
            ("st_isvalid", Err(_), _) => return "false".into(),
            ("st_isvalid", _, Some(true)) => return "true".into(),
            ("st_convexhull", _, Some(true)) => return input_rendered.clone(),
            ("st_orientedenvelope", _, Some(true)) => return "POLYGON EMPTY".into(),
            ("st_simplify", _, Some(true)) => return input_rendered.clone(),
            ("st_length", Ok(g), _) => {
                return geos_linear_length(g).map_or_else(|e| format!("ERROR: {e}"), f);
            }
            _ => {}
        }
        geos_raw_out.clone()
    }));

    [geo_raw_out, geo_norm_out, geos_raw_out.clone(), geos_wrap_out]
}

// ------------------------------------------------------------------------------------------
// Binary evaluation
// ------------------------------------------------------------------------------------------

fn eval_binary(func: &str, wa: &[u8], wb: &[u8]) -> [String; 4] {
    let (a, bb) = match (wkb::reader::read_wkb(wa), wkb::reader::read_wkb(wb)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let s = "ERROR: wkb".to_string();
            return [s.clone(), s.clone(), s.clone(), s];
        }
    };
    let (ra, rb) = (geo_raw(&a), geo_raw(&bb));
    let (na, nb) = (norm::to_geo(&a), norm::to_geo(&bb));
    let ga = geos::Geometry::new_from_wkb(wa).map_err(|e| format!("ERROR: {e}"));
    let gb = geos::Geometry::new_from_wkb(wb).map_err(|e| format!("ERROR: {e}"));

    let geo_raw_out = flatten(run(|| {
        let (a, b_) = match (&ra, &rb) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return format!("ERROR: {e}"),
        };
        match func {
            "st_distance" => f(Euclidean.distance(a, b_)),
            "st_contains" => b(a.relate(b_).is_contains()),
            "st_intersects" => b(a.relate(b_).is_intersects()),
            "st_within" => b(a.relate(b_).is_within()),
            "st_touches" => b(a.relate(b_).is_touches()),
            "st_relate" => im_string(&a.relate(b_)),
            _ => unreachable!(),
        }
    }));

    let geo_norm_out = flatten(run(|| {
        let empty_gc: Geometry = Geometry::GeometryCollection(GeometryCollection::<f64>::new_from(vec![]));
        match func {
            "st_relate" => {
                let a = na.as_ref().unwrap_or(&empty_gc);
                let b_ = nb.as_ref().unwrap_or(&empty_gc);
                im_string(&a.relate(b_))
            }
            _ => {
                let (Some(a), Some(b_)) = (&na, &nb) else {
                    return if func == "st_distance" { "NULL".into() } else { "false".into() };
                };
                match func {
                    "st_distance" => f(Euclidean.distance(a, b_)),
                    "st_contains" => b(a.relate(b_).is_contains()),
                    "st_intersects" => b(a.relate(b_).is_intersects()),
                    "st_within" => b(a.relate(b_).is_within()),
                    "st_touches" => b(a.relate(b_).is_touches()),
                    _ => unreachable!(),
                }
            }
        }
    }));

    let geos_raw_out = flatten(run(|| {
        let (a, b_) = match (&ga, &gb) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return e.clone(),
        };
        let r: Result<String, geos::Error> = (|| {
            Ok(match func {
                "st_distance" => f(a.distance(b_)?),
                "st_contains" => b(a.contains(b_)?),
                "st_intersects" => b(a.intersects(b_)?),
                "st_within" => b(a.within(b_)?),
                "st_touches" => b(a.touches(b_)?),
                "st_relate" => relate_isolated(a, b_, wa, wb)?,
                _ => unreachable!(),
            })
        })();
        r.unwrap_or_else(|e| format!("ERROR: {e}"))
    }));

    let geos_wrap_out = flatten(run(|| {
        let empty = |g: &Result<geos::Geometry, String>| {
            g.as_ref().ok().map(|g| g.is_empty().unwrap_or(false)).unwrap_or(false)
        };
        if func == "st_distance" && (empty(&ga) || empty(&gb)) {
            return "NULL".into();
        }
        if func != "st_relate" && func != "st_distance" && (empty(&ga) || empty(&gb)) {
            return "false".into();
        }
        geos_raw_out.clone()
    }));

    [geo_raw_out, geo_norm_out, geos_raw_out.clone(), geos_wrap_out]
}

/// GEOS 3.14.1 segfaults on some EMPTY combinations (as PostGIS does), so relate inputs that
/// contain EMPTY are evaluated in a child process.
fn relate_isolated(a: &geos::Geometry, b: &geos::Geometry, wa: &[u8], wb: &[u8]) -> Result<String, geos::Error> {
    let has_empty = |w: &[u8]| has_empty(&render_wkb(w));
    if !has_empty(wa) && !has_empty(wb) {
        return a.relate(b);
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--geos-relate")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let input = format!("{}\n{}\n", hex::encode(wa), hex::encode(wb));
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        return Ok(format!("ERROR: GEOS crashed ({})", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ------------------------------------------------------------------------------------------
// Comparison and classification
// ------------------------------------------------------------------------------------------

fn is_err(s: &str) -> bool {
    s.starts_with("ERROR") || s.starts_with("PANIC")
}

fn geom_type(s: &str) -> &str {
    s.split(|c: char| c == '(' || c == ' ').next().unwrap_or("")
}

fn coord_tokens(s: &str) -> Vec<String> {
    let body: String = s
        .chars()
        .map(|c| if c == '(' || c == ')' { ',' } else { c })
        .collect();
    body.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-'))
        .map(|t| t.to_string())
        .collect()
}

fn numbers(s: &str) -> Vec<f64> {
    coord_tokens(s)
        .iter()
        .flat_map(|t| t.split_whitespace().map(|n| n.parse::<f64>().unwrap_or(f64::NAN)).collect::<Vec<_>>())
        .collect()
}

fn skeleton(s: &str) -> String {
    s.chars().filter(|c| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e')).collect()
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale.max(a.abs()).max(b.abs())
}

/// The kind of a disagreement, from the rendered values alone.
fn classify(func: &str, pg: &str, got: &str) -> String {
    if got == NA {
        return "not available".into();
    }
    if got.starts_with("PANIC") {
        return "backend panic".into();
    }
    if is_err(got) && !is_err(pg) {
        return "backend error".into();
    }
    if is_err(pg) && !is_err(got) {
        return "PostGIS error".into();
    }
    if pg == "NULL" || got == "NULL" {
        return "NULL vs value".into();
    }
    match func {
        "st_area" | "st_length" | "st_distance" => {
            let (a, b_) = (pg.parse::<f64>().unwrap_or(f64::NAN), got.parse::<f64>().unwrap_or(f64::NAN));
            if close(a, b_, 0.0) { "float precision".into() } else { "different value".into() }
        }
        "st_isvalid" | "st_contains" | "st_intersects" | "st_within" | "st_touches" => "different boolean".into(),
        "st_relate" => "different matrix".into(),
        _ => {
            if geom_type(pg) != geom_type(got) {
                return "different geometry type".into();
            }
            let (ta, tb) = (coord_tokens(pg), coord_tokens(got));
            let mut sa = ta.clone();
            let mut sb = tb.clone();
            sa.sort();
            sa.dedup();
            sb.sort();
            sb.dedup();
            if ta.len() == tb.len() && sa == sb {
                return "vertex order".into();
            }
            if skeleton(pg) == skeleton(got) {
                let (na, nb) = (numbers(pg), numbers(got));
                let scale = na.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                if na.len() == nb.len() && na.iter().zip(&nb).all(|(a, b_)| close(*a, *b_, scale)) {
                    return "float precision".into();
                }
            }
            if ta.len() != tb.len() {
                "different vertices (count)".into()
            } else {
                "different vertices (coordinates)".into()
            }
        }
    }
}

#[derive(Serialize)]
struct Disagreement {
    func: String,
    backend: String,
    id: i64,
    src: String,
    kind: String,
    empty_input: bool,
    invalid_input: bool,
    geos_agrees: bool,
    npoints: usize,
    input: String,
    pg: String,
    got: String,
}

#[derive(Default, Serialize)]
struct Stats {
    total: usize,
    agree: usize,
    by_kind: BTreeMap<String, usize>,
    by_src: BTreeMap<String, [usize; 2]>,
    /// Inputs that are non-empty and valid in PostGIS, and the disagreements among them.
    total_valid_nonempty: usize,
    disagree_valid_nonempty: usize,
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut k = n;
        while !s.is_char_boundary(k) {
            k -= 1;
        }
        format!("{}…[{} chars]", &s[..k], s.len())
    }
}

fn read_tsv(path: PathBuf) -> Vec<Vec<String>> {
    let f = fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    BufReader::new(f)
        .lines()
        .map(|l| l.unwrap().split('\t').map(str::to_string).collect())
        .collect()
}

fn read_pg(func: &str) -> HashMap<i64, String> {
    read_tsv(data_dir().join("pg").join(format!("{func}.tsv")))
        .into_iter()
        .map(|r| (r[0].parse().unwrap(), r[1..].join("\t")))
        .collect()
}

struct Case {
    id: i64,
    src: String,
    wkb: Vec<Vec<u8>>,
    tol: f64,
    vwtol: f64,
    invalid: bool,
    empty: bool,
    npoints: usize,
    input: String,
}

fn has_empty(rendered: &str) -> bool {
    // The harness renders an EMPTY member of a multi-geometry as `()`.
    rendered.contains("EMPTY") || rendered.contains("()")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--geos-relate") {
        let lines: Vec<String> = std::io::stdin().lines().map(Result::unwrap).collect();
        let a = geos::Geometry::new_from_wkb(&hex::decode(&lines[0]).unwrap()).unwrap();
        let b = geos::Geometry::new_from_wkb(&hex::decode(&lines[1]).unwrap()).unwrap();
        match a.relate(&b) {
            Ok(s) => println!("{s}"),
            Err(e) => println!("ERROR: {e}"),
        }
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));
    println!("GEOS {}", geos::version().unwrap());
    let dir = data_dir();
    let out_dir = dir.join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let only: Option<Vec<String>> = std::env::args().nth(1).map(|s| s.split(',').map(str::to_string).collect());

    let valid_u: HashMap<i64, bool> = read_tsv(dir.join("pg/_valid_unary.tsv"))
        .into_iter()
        .map(|r| (r[0].parse().unwrap(), r[1] == "true"))
        .collect();
    let valid_p: HashMap<i64, bool> = read_tsv(dir.join("pg/_valid_pair.tsv"))
        .into_iter()
        .map(|r| (r[0].parse().unwrap(), r[1] == "true" && r[2] == "true"))
        .collect();

    let unary: Vec<Case> = read_tsv(dir.join("unary.tsv"))
        .into_iter()
        .map(|r| {
            let id: i64 = r[0].parse().unwrap();
            let w = hex::decode(&r[2]).unwrap();
            let input = render_wkb(&w);
            Case {
                id,
                src: r[1].clone(),
                tol: r[3].parse().unwrap(),
                vwtol: r[4].parse().unwrap(),
                invalid: !valid_u[&id],
                empty: has_empty(&input),
                npoints: coord_tokens(&input).len(),
                input,
                wkb: vec![w],
            }
        })
        .collect();
    let pairs: Vec<Case> = read_tsv(dir.join("pairs.tsv"))
        .into_iter()
        .map(|r| {
            let id: i64 = r[0].parse().unwrap();
            let (wa, wb) = (hex::decode(&r[2]).unwrap(), hex::decode(&r[3]).unwrap());
            let (ia, ib) = (render_wkb(&wa), render_wkb(&wb));
            Case {
                id,
                src: r[1].clone(),
                tol: 0.0,
                vwtol: 0.0,
                invalid: !valid_p[&id],
                empty: has_empty(&ia) || has_empty(&ib),
                npoints: coord_tokens(&ia).len() + coord_tokens(&ib).len(),
                input: format!("A: {}\nB: {}", trunc(&ia, 600), trunc(&ib, 600)),
                wkb: vec![wa, wb],
            }
        })
        .collect();

    fs::create_dir_all(out_dir.join("per_function")).unwrap();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

    for func in UNARY.iter().chain(BINARY.iter()) {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == func)) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let pg = read_pg(func);
        let is_unary = UNARY.contains(func);
        let cases = if is_unary { &unary } else { &pairs };
        let chunk = cases.len().div_ceil(threads);
        let results: Vec<(i64, [String; 4])> = std::thread::scope(|s| {
            let handles: Vec<_> = cases
                .chunks(chunk)
                .map(|cs| {
                    s.spawn(move || {
                        cs.iter()
                            .map(|c| {
                                let r = if is_unary {
                                    eval_unary(func, &UnaryInput { wkb: &c.wkb[0], tol: c.tol, vwtol: c.vwtol })
                                } else {
                                    eval_binary(func, &c.wkb[0], &c.wkb[1])
                                };
                                (c.id, r)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        let mut stats: BTreeMap<String, Stats> = BTreeMap::new();
        let mut dis_out = fs::File::create(out_dir.join(format!("per_function/{func}.jsonl"))).unwrap();
        for (c, (id, outs)) in cases.iter().zip(results) {
            assert_eq!(c.id, id);
            let pgv = render_pg(func, &pg[&id]);
            let geos_agrees = outs[2] == pgv || (is_err(&outs[2]) && is_err(&pgv));
            for (k, got) in outs.iter().enumerate() {
                let st = stats.entry(BACKENDS[k].to_string()).or_default();
                st.total += 1;
                if !c.empty && !c.invalid {
                    st.total_valid_nonempty += 1;
                }
                let agree = got == &pgv || (is_err(got) && is_err(&pgv) && !got.starts_with("PANIC"));
                let e = st.by_src.entry(c.src.clone()).or_default();
                e[0] += 1;
                if agree {
                    st.agree += 1;
                    continue;
                }
                e[1] += 1;
                let kind = classify(func, &pgv, got);
                *st.by_kind.entry(kind.clone()).or_default() += 1;
                if !c.empty && !c.invalid {
                    st.disagree_valid_nonempty += 1;
                }
                let d = Disagreement {
                    func: func.to_string(),
                    backend: BACKENDS[k].to_string(),
                    id,
                    src: c.src.clone(),
                    kind,
                    empty_input: c.empty,
                    invalid_input: c.invalid,
                    geos_agrees,
                    npoints: c.npoints,
                    input: trunc(&c.input, 1300),
                    pg: trunc(&pgv, 600),
                    got: trunc(got, 600),
                };
                writeln!(dis_out, "{}", serde_json::to_string(&d).unwrap()).unwrap();
            }
        }
        let line: Vec<String> = BACKENDS
            .iter()
            .map(|b_| {
                let s = &stats[*b_];
                format!("{b_} {}/{} ({:.3}%)", s.agree, s.total, 100.0 * s.agree as f64 / s.total as f64)
            })
            .collect();
        println!("{func:22} {}  [{:.1}s]", line.join("  "), t0.elapsed().as_secs_f64());
        fs::write(out_dir.join(format!("per_function/{func}.json")), serde_json::to_string_pretty(&stats).unwrap()).unwrap();
    }
}
