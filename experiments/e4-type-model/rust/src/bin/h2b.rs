//! H2b: SQL constructs x pairs of geometry-valued inputs, native outputs vs "all WKB".
//!
//! Native mode uses each expression as geodatafusion produces it today. WKB mode wraps every
//! geometry-valued expression in `to_wkb(..)` (ST_AsBinary: Binary tagged geoarrow.wkb with
//! the input CRS), which simulates "every geometry-returning function returns WKB".

use e4_type_model::{context, run};

struct Src {
    key: &'static str,
    /// Constant expression of the same type (for VALUES), if one can be written.
    konst: Option<&'static str>,
    /// Expression with `{q}` for a table qualifier (`t1.`, `t2.` or empty).
    expr: &'static str,
}

const SRCS: &[Src] = &[
    Src { key: "P", konst: Some("ST_Centroid(ST_GeomFromText('LINESTRING(0 0,2 4)'))"), expr: "ST_Centroid({q}wkb)" },
    Src { key: "G", konst: Some("ST_GeomFromText('POINT(1 2)')"), expr: "ST_GeomFromText(ST_AsText({q}wkb))" },
    Src { key: "W", konst: Some("ST_AsBinary(ST_GeomFromText('POINT(1 2)'))"), expr: "{q}wkb" },
    Src { key: "PS", konst: Some("ST_Point(1.0, 2.0)"), expr: "{q}pt_sep" },
    Src { key: "PI", konst: None, expr: "{q}pt_il" },
    Src { key: "PZ", konst: Some("ST_PointZ(1.0, 2.0, 3.0)"), expr: "{q}pt_z" },
    Src { key: "W4326", konst: Some("ST_AsBinary(ST_Point(1.0, 2.0, 4326))"), expr: "{q}wkb4326" },
];

fn src(key: &str) -> &'static Src {
    SRCS.iter().find(|s| s.key == key).unwrap()
}

fn e(key: &str, q: &str, wkb: bool) -> String {
    let base = src(key).expr.replace("{q}", q);
    if wkb { format!("ST_AsBinary({base})") } else { base }
}

const PAIRS: &[(&str, &str, &str)] = &[
    ("P", "PS", "control: two native XY separated points (centroid vs point column)"),
    ("P", "G", "Point (ST_Centroid) vs Geometry (ST_GeomFromText)"),
    ("P", "W", "native Point vs WKB column"),
    ("G", "W", "native Geometry vs WKB column"),
    ("P", "PI", "Point separated vs Point interleaved"),
    ("P", "PZ", "Point XY vs Point XYZ"),
    ("W", "W4326", "WKB no CRS vs WKB EPSG:4326 (same storage, different CRS)"),
];

fn constructs(a: &str, b: &str, wkb: bool) -> Vec<(&'static str, String)> {
    let a0 = e(a, "", wkb);
    let b0 = e(b, "", wkb);
    let a1 = e(a, "t1.", wkb);
    let b2 = e(b, "t2.", wkb);
    vec![
        (
            "UNION ALL",
            format!("SELECT ST_AsText(g) FROM (SELECT {a0} AS g FROM t UNION ALL SELECT {b0} AS g FROM t)"),
        ),
        (
            "CASE",
            format!("SELECT ST_AsText(CASE WHEN id = 1 THEN {a0} ELSE {b0} END) FROM t"),
        ),
        ("COALESCE", format!("SELECT ST_AsText(COALESCE({a0}, {b0})) FROM t")),
        (
            "VALUES (2 rows)",
            match (src(a).konst, src(b).konst) {
                (Some(ka), Some(kb)) => {
                    let w = |k: &str| if wkb { format!("ST_AsBinary({k})") } else { k.to_string() };
                    format!("SELECT ST_AsText(column1) FROM (VALUES ({}), ({}))", w(ka), w(kb))
                }
                _ => "SELECT 'n/a: no constant constructor for this type' AS skipped".to_string(),
            },
        ),
        (
            "IN (subquery)",
            format!("SELECT t1.id FROM t AS t1 WHERE {a1} IN (SELECT {b2} FROM t AS t2) ORDER BY t1.id"),
        ),
        (
            "make_array",
            format!("SELECT ST_AsText(x) FROM (SELECT unnest(make_array({a0}, {b0})) AS x FROM t)"),
        ),
        (
            "array_agg (of UNION ALL)",
            format!(
                "SELECT ST_AsText(x) FROM (SELECT unnest(arr) AS x FROM (SELECT array_agg(g) AS arr FROM (SELECT {a0} AS g FROM t UNION ALL SELECT {b0} AS g FROM t)))"
            ),
        ),
        (
            "JOIN ON =",
            format!("SELECT t1.id, t2.id FROM t AS t1 JOIN t AS t2 ON {a1} = {b2} ORDER BY 1, 2"),
        ),
        ("= (projection)", format!("SELECT id, {a0} = {b0} FROM t ORDER BY id")),
    ]
}

#[tokio::main]
async fn main() {
    let ctx = context();

    println!("## Source expressions and their output types\n");
    println!("| key | expression | native type | WKB-mode type |");
    println!("|---|---|---|---|");
    for s in SRCS {
        let n = run(&ctx, &format!("SELECT {} AS g FROM t", e(s.key, "", false))).await;
        let w = run(&ctx, &format!("SELECT {} AS g FROM t", e(s.key, "", true))).await;
        println!(
            "| {} | `{}` | {} | {} |",
            s.key,
            src(s.key).expr.replace("{q}", ""),
            n.fields.join(", "),
            w.fields.join(", ")
        );
    }

    // Single-type controls for constructs that may lose extension metadata on their own.
    println!("\n## Same-source controls (a construct over one source with itself)\n");
    println!("| source | construct | native | WKB mode |");
    println!("|---|---|---|---|");
    for s in ["P", "G", "W", "W4326"] {
        let cn = constructs(s, s, false);
        let cw = constructs(s, s, true);
        for ((name, qn), (_, qw)) in cn.iter().zip(cw.iter()) {
            let n = run(&ctx, qn).await;
            let w = run(&ctx, qw).await;
            println!("| {s} | {name} | {} | {} |", cell(&n), cell(&w));
        }
    }

    let mut summary = vec![];
    println!("\n## Matrix\n");
    for (a, b, desc) in PAIRS {
        println!("### {a} vs {b}: {desc}\n");
        println!("| construct | native | WKB mode |");
        println!("|---|---|---|");
        let cn = constructs(a, b, false);
        let cw = constructs(a, b, true);
        for ((name, qn), (_, qw)) in cn.iter().zip(cw.iter()) {
            let n = run(&ctx, qn).await;
            let w = run(&ctx, qw).await;
            println!("| {name} | {} | {} |", cell(&n), cell(&w));
            summary.push((format!("{a}-{b}"), *name, n.ok, w.ok));
        }
        println!();
    }

    println!("## Summary (FAIL = error; values are checked by hand in the report)\n");
    let names: Vec<&str> = constructs("P", "P", false).iter().map(|c| c.0).collect();
    print!("| pair |");
    for n in &names {
        print!(" {n} |");
    }
    println!();
    print!("|---|");
    for _ in &names {
        print!("---|");
    }
    println!();
    for (a, b, _) in PAIRS {
        let key = format!("{a}-{b}");
        print!("| {key} |");
        for n in &names {
            let (_, _, nok, wok) = summary.iter().find(|s| s.0 == key && s.1 == *n).unwrap();
            let f = |ok: &bool| if *ok { "ok" } else { "FAIL" };
            print!(" {} / {} |", f(nok), f(wok));
        }
        println!();
    }

    println!("\n## Queries used (native mode, pair P vs W)\n");
    for (name, q) in constructs("P", "W", false) {
        println!("- {name}: `{q}`");
    }
    println!("\nWKB mode wraps every source expression in `ST_AsBinary(..)`.");
}

fn cell(o: &e4_type_model::Outcome) -> String {
    if o.ok {
        format!("ok: {} → {}", o.fields.join(", "), o.detail)
    } else {
        format!("**FAIL ({})**: {}", o.stage, o.detail)
    }
}
