//! Engine-independent rendering of result values.
//!
//! Both engines (geodatafusion and PostGIS) render their results through these functions so
//! that `.slt` expectations recorded against PostGIS can be compared with geodatafusion output
//! without either side's native text formatting leaking into the comparison.
//!
//! Note that this only applies to values whose SQL/Arrow type is a geometry, float, etc. A
//! function that returns *text* (e.g. `ST_AsText`) is compared verbatim, so text formatting
//! differences there are real parity failures.

use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
};

pub const NULL: &str = "NULL";
pub const EMPTY_STRING: &str = "(empty)";

/// Number of significant digits floats are rounded to before comparison.
///
/// geo and GEOS can legitimately differ in the last few ulps, so we don't compare full precision.
const SIGNIFICANT_DIGITS: usize = 12;

/// Render a float rounded to [`SIGNIFICANT_DIGITS`], in shortest round-trip form.
pub fn float(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let rounded: f64 = format!("{:.*e}", SIGNIFICANT_DIGITS - 1, v)
        .parse()
        .expect("a formatted float parses");
    if rounded == 0.0 {
        // Normalise negative zero, so that -0 and 0 compare equal.
        return "0".to_string();
    }
    rounded.to_string()
}

/// Render a text value, escaping characters that would break the line-based `.slt` format.
pub fn text(s: &str) -> String {
    if s.is_empty() {
        return EMPTY_STRING.to_string();
    }
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Render binary (non-geometry) data the way Postgres renders `bytea`.
pub fn bytes(b: &[u8]) -> String {
    let mut out = String::with_capacity(2 + b.len() * 2);
    out.push_str("\\x");
    for byte in b {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("\\x").unwrap_or(s);
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Render (E)WKB bytes as canonical EWKT: `SRID=<srid>;<ISO WKT>`, SRID omitted when 0/absent.
pub fn ewkb(buf: &[u8], srid: Option<i32>) -> String {
    let srid = srid.or_else(|| ewkb_srid(buf)).filter(|s| *s != 0);
    let geom_wkt = match wkb::reader::read_wkb(buf) {
        Ok(geom) => {
            let mut out = String::new();
            write_geometry(&mut out, &geom);
            out
        }
        // e.g. curved geometry types, which the wkb crate does not support.
        Err(e) => format!("<unparseable wkb ({e}): {}>", bytes(buf)),
    };
    match srid {
        Some(srid) => format!("SRID={srid};{geom_wkt}"),
        None => geom_wkt,
    }
}

/// Extract the SRID embedded in an EWKB buffer, if any.
fn ewkb_srid(buf: &[u8]) -> Option<i32> {
    let little_endian = *buf.first()? == 1;
    let read_u32 = |b: &[u8]| -> Option<u32> {
        let arr: [u8; 4] = b.try_into().ok()?;
        Some(if little_endian {
            u32::from_le_bytes(arr)
        } else {
            u32::from_be_bytes(arr)
        })
    };
    let type_code = read_u32(buf.get(1..5)?)?;
    if type_code & 0x2000_0000 != 0 {
        read_u32(buf.get(5..9)?).map(|v| v as i32)
    } else {
        None
    }
}

/// Render a bounding box the way PostGIS renders `box2d`/`box3d`.
pub fn rect(r: &impl RectTrait<T = f64>) -> String {
    let (min, max) = (r.min(), r.max());
    match r.dim() {
        Dimensions::Xyz | Dimensions::Xyzm => format!(
            "BOX3D({} {} {},{} {} {})",
            float(min.x()),
            float(min.y()),
            float(min.nth_or_panic(2)),
            float(max.x()),
            float(max.y()),
            float(max.nth_or_panic(2)),
        ),
        _ => format!(
            "BOX({} {},{} {})",
            float(min.x()),
            float(min.y()),
            float(max.x()),
            float(max.y()),
        ),
    }
}

/// Normalize a Postgres `box2d`/`box3d` text value (`BOX(1 2,3 4)`) to the [`rect`] format.
pub fn pg_box(s: &str) -> String {
    let Some((prefix, rest)) = s.split_once('(') else {
        return text(s);
    };
    let inner = rest.trim_end_matches(')');
    let corners: Vec<String> = inner
        .split(',')
        .map(|corner| {
            corner
                .split_whitespace()
                .map(|n| {
                    n.parse::<f64>()
                        .map(float)
                        .unwrap_or_else(|_| n.to_string())
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    format!("{prefix}({})", corners.join(","))
}

fn dim_tag(dim: Dimensions) -> &'static str {
    match dim {
        Dimensions::Xy | Dimensions::Unknown(2) => "",
        Dimensions::Xyz | Dimensions::Unknown(3) => " Z",
        Dimensions::Xym => " M",
        Dimensions::Xyzm | Dimensions::Unknown(_) => " ZM",
    }
}

fn write_coord(out: &mut String, c: &impl CoordTrait<T = f64>) {
    let n = c.dim().size();
    for i in 0..n {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&float(c.nth_or_panic(i)));
    }
}

fn write_coords<'a, C: CoordTrait<T = f64> + 'a>(
    out: &mut String,
    coords: impl Iterator<Item = C>,
) {
    out.push('(');
    for (i, c) in coords.enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_coord(out, &c);
    }
    out.push(')');
}

fn write_polygon_body(out: &mut String, p: &impl PolygonTrait<T = f64>) {
    out.push('(');
    if let Some(ext) = p.exterior() {
        write_coords(out, ext.coords());
        for int in p.interiors() {
            out.push(',');
            write_coords(out, int.coords());
        }
    }
    out.push(')');
}

fn point_is_empty(p: &impl PointTrait<T = f64>) -> bool {
    match p.coord() {
        None => true,
        Some(c) => c.x().is_nan() && c.y().is_nan(),
    }
}

/// Write ISO WKT for any geometry.
pub fn write_geometry(out: &mut String, g: &impl GeometryTrait<T = f64>) {
    let tag = dim_tag(g.dim());
    match g.as_type() {
        GeometryType::Point(p) => {
            out.push_str("POINT");
            out.push_str(tag);
            match p.coord() {
                Some(c) if !point_is_empty(p) => {
                    out.push_str(if tag.is_empty() { "(" } else { " (" });
                    write_coord(out, &c);
                    out.push(')');
                }
                _ => out.push_str(" EMPTY"),
            }
        }
        GeometryType::LineString(ls) => {
            out.push_str("LINESTRING");
            out.push_str(tag);
            if ls.num_coords() == 0 {
                out.push_str(" EMPTY");
            } else {
                if !tag.is_empty() {
                    out.push(' ');
                }
                write_coords(out, ls.coords());
            }
        }
        GeometryType::Polygon(p) => {
            out.push_str("POLYGON");
            out.push_str(tag);
            if p.exterior().is_none_or(|e| e.num_coords() == 0) {
                out.push_str(" EMPTY");
            } else {
                if !tag.is_empty() {
                    out.push(' ');
                }
                write_polygon_body(out, p);
            }
        }
        GeometryType::MultiPoint(mp) => {
            out.push_str("MULTIPOINT");
            out.push_str(tag);
            if mp.num_points() == 0 {
                out.push_str(" EMPTY");
            } else {
                out.push_str(if tag.is_empty() { "(" } else { " (" });
                for (i, p) in mp.points().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    match p.coord() {
                        Some(c) if !point_is_empty(&p) => write_coords(out, std::iter::once(c)),
                        _ => out.push_str("EMPTY"),
                    }
                }
                out.push(')');
            }
        }
        GeometryType::MultiLineString(mls) => {
            out.push_str("MULTILINESTRING");
            out.push_str(tag);
            if mls.num_line_strings() == 0 {
                out.push_str(" EMPTY");
            } else {
                out.push_str(if tag.is_empty() { "(" } else { " (" });
                for (i, ls) in mls.line_strings().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_coords(out, ls.coords());
                }
                out.push(')');
            }
        }
        GeometryType::MultiPolygon(mp) => {
            out.push_str("MULTIPOLYGON");
            out.push_str(tag);
            if mp.num_polygons() == 0 {
                out.push_str(" EMPTY");
            } else {
                out.push_str(if tag.is_empty() { "(" } else { " (" });
                for (i, p) in mp.polygons().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_polygon_body(out, &p);
                }
                out.push(')');
            }
        }
        GeometryType::GeometryCollection(gc) => {
            out.push_str("GEOMETRYCOLLECTION");
            out.push_str(tag);
            if gc.num_geometries() == 0 {
                out.push_str(" EMPTY");
            } else {
                out.push_str(if tag.is_empty() { "(" } else { " (" });
                for (i, child) in gc.geometries().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_geometry(out, &child);
                }
                out.push(')');
            }
        }
        GeometryType::Rect(r) => out.push_str(&rect(r)),
        GeometryType::Triangle(_) => out.push_str("<unsupported: TRIANGLE>"),
        GeometryType::Line(_) => out.push_str("<unsupported: LINE>"),
    }
}
