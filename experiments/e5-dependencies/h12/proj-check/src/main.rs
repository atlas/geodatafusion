//! H12 step 2: transform every coordinate in cases.tsv with PROJ (through the `proj` crate and,
//! for Z and inverse pipelines, through `proj-sys` directly) and compare with PostGIS's output.
//!
//! Usage: proj-check <cases.tsv> <results.tsv>

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;

use proj::Proj;
use proj_sys::*;

const SIG: usize = 12;

fn round_sig(v: f64) -> f64 {
    format!("{:.*e}", SIG - 1, v).parse().unwrap()
}

fn agree(a: f64, b: f64) -> bool {
    // Same rule as the slt harness (render::float): equal after rounding to 12 significant digits,
    // with -0 == 0.
    round_sig(a) == round_sig(b)
}

fn rel(a: f64, b: f64) -> f64 {
    if a == b {
        0.0
    } else {
        (a - b).abs() / a.abs().max(b.abs())
    }
}

/// Raw PROJ handle mirroring PostGIS: crs_to_crs (or a pipeline) + normalize_for_visualization.
struct Raw {
    ctx: *mut PJ_CONTEXT,
    pj: *mut PJ,
}

impl Raw {
    fn new(mode: &str, from: &str, to: &str) -> Raw {
        unsafe {
            let ctx = proj_context_create();
            let f = CString::new(from).unwrap();
            let pj = if mode == "crs" {
                let t = CString::new(to).unwrap();
                proj_create_crs_to_crs(ctx, f.as_ptr(), t.as_ptr(), std::ptr::null_mut())
            } else {
                proj_create(ctx, f.as_ptr())
            };
            assert!(!pj.is_null(), "proj_create failed for {from} -> {to}");
            // PostGIS keeps the operation as is when it can't be normalised (a bare conversion
            // such as EPSG::16031 has no source/target CRS).
            let norm = proj_normalize_for_visualization(ctx, pj);
            if norm.is_null() {
                eprintln!("  (normalize_for_visualization failed for {from}; using it unnormalised)");
                proj_errno_reset(pj);
                return Raw { ctx, pj };
            }
            proj_destroy(pj);
            Raw { ctx, pj: norm }
        }
    }

    fn trans(&self, inv: bool, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        unsafe {
            proj_errno_reset(self.pj);
            let dir = if inv { PJ_DIRECTION_PJ_INV } else { PJ_DIRECTION_PJ_FWD };
            // As PostGIS (ptarray_transform): degrees in/out when the operation is angular.
            let (x, y) = if proj_angular_input(self.pj, dir) != 0 {
                (x.to_radians(), y.to_radians())
            } else {
                (x, y)
            };
            let c = proj_trans(
                self.pj,
                dir,
                PJ_COORD { xyzt: PJ_XYZT { x, y, z, t: f64::INFINITY } },
            );
            let e = proj_errno(self.pj);
            assert_eq!(e, 0, "proj_trans error {e}");
            let (ox, oy) = if proj_angular_output(self.pj, dir) != 0 {
                (c.xyzt.x.to_degrees(), c.xyzt.y.to_degrees())
            } else {
                (c.xyzt.x, c.xyzt.y)
            };
            (ox, oy, c.xyzt.z)
        }
    }

    fn describe(&self) -> String {
        unsafe {
            let p = proj_pj_info(self.pj);
            let s = |c: *const std::os::raw::c_char| {
                if c.is_null() { String::new() } else { CStr::from_ptr(c).to_string_lossy().into_owned() }
            };
            format!("{} | {}", s(p.description), s(p.definition))
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe {
            proj_destroy(self.pj);
            proj_context_destroy(self.ctx);
        }
    }
}

fn parse(s: &str) -> Option<f64> {
    if s.is_empty() { None } else { Some(s.parse().unwrap()) }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = fs::read_to_string(&args[1]).unwrap();
    let info = unsafe { proj_info() };
    let cs = |c: *const std::os::raw::c_char| unsafe { CStr::from_ptr(c).to_string_lossy().into_owned() };
    eprintln!("PROJ release: {}", cs(info.release));
    eprintln!("PROJ searchpath: {}", cs(info.searchpath));
    unsafe {
        eprintln!("network enabled (default ctx): {}", proj_context_is_network_enabled(std::ptr::null_mut()));
    }

    let mut out = String::from(
        "case\tidx\tcrate_x\tcrate_y\tsys_x\tsys_y\tsys_z\tpg_x\tpg_y\tpg_z\tmax_rel\tagree12\n",
    );
    let mut raws: HashMap<String, Raw> = HashMap::new();
    let mut crates: HashMap<String, Option<Proj>> = HashMap::new();
    let (mut n, mut ok) = (0, 0);
    let mut cases_bad: Vec<String> = vec![];
    for line in input.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let (case, mode, from, to, idx) = (f[0], f[1], f[2], f[3], f[4]);
        let (x, y, z) = (parse(f[5]).unwrap(), parse(f[6]).unwrap(), parse(f[7]));
        let (px, py, pz) = (parse(f[8]).unwrap(), parse(f[9]).unwrap(), parse(f[10]));
        let raw = raws.entry(case.to_string()).or_insert_with(|| {
            let r = Raw::new(mode, from, to);
            eprintln!("{case}: {}", r.describe());
            r
        });
        let (sx, sy, sz) = raw.trans(mode == "pipe_inv", x, y, z.unwrap_or(0.0));
        // The `proj` crate API that geodatafusion would use: 2D, forward only.
        let crate_proj = crates.entry(case.to_string()).or_insert_with(|| match mode {
            "crs" => Some(Proj::new_known_crs(from, to, None).unwrap()),
            "pipe_fwd" => {
                // What the plan proposes for ST_TransformPipeline: Proj::new(pipeline) + convert.
                // Reported only; it neither normalises axes nor converts degrees to radians.
                let p = Proj::new(from).unwrap();
                eprintln!("  Proj::new({from}).convert(({x}, {y})) = {:?}", p.convert((x, y)));
                eprintln!("  Proj::new({from}).project(radians, fwd) = {:?}", p.project((x.to_radians(), y.to_radians()), false));
                None
            }
            _ => None,
        });
        let (cx, cy) = match crate_proj {
            Some(p) => p.convert((x, y)).unwrap(),
            None => (f64::NAN, f64::NAN),
        };
        let mut good = agree(sx, px) && agree(sy, py);
        let mut mr = rel(sx, px).max(rel(sy, py));
        if let Some(pz) = pz {
            good &= agree(sz, pz);
            mr = mr.max(rel(sz, pz));
        }
        if crate_proj.is_some() {
            good &= cx == sx && cy == sy;
        }
        n += 1;
        if good {
            ok += 1;
        } else if !cases_bad.contains(&case.to_string()) {
            cases_bad.push(case.to_string());
        }
        out.push_str(&format!(
            "{case}\t{idx}\t{cx:?}\t{cy:?}\t{sx:?}\t{sy:?}\t{sz:?}\t{px:?}\t{py:?}\t{}\t{mr:e}\t{good}\n",
            pz.map(|v| format!("{v:?}")).unwrap_or_default()
        ));
    }
    fs::write(&args[2], out).unwrap();
    println!("coordinates agreeing to {SIG} significant digits: {ok}/{n}");
    println!("cases with a mismatch: {cases_bad:?}");
}
