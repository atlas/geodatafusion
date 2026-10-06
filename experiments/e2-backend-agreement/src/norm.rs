//! The cheap, deterministic PostGIS rules the G2 plan (R6, §7) proposes on top of `geo`.

use geo::{
    Area, Centroid, ConvexHull, Distance as _, Euclidean, InteriorPoint, Length, MinimumRotatedRect,
    Simplify, SimplifyVw, Validation,
};
use geo_types::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::{render_geo, NA};

/// Converts to `geo`, dropping EMPTY parts (`POINT EMPTY` in a collection, empty rings/lines).
/// `None` means the whole geometry is EMPTY (G2's `GeoValue::Empty`).
pub fn to_geo(w: &impl geo_traits::GeometryTrait<T = f64>) -> Option<Geometry> {
    use geo_traits::*;
    fn coords<'a>(l: &impl LineStringTrait<T = f64>) -> LineString {
        LineString::new(l.coords().map(|c| Coord { x: c.x(), y: c.y() }).collect())
    }
    fn poly(p: &impl PolygonTrait<T = f64>) -> Option<Polygon> {
        let ext = coords(&p.exterior()?);
        if ext.0.is_empty() {
            return None;
        }
        Some(Polygon::new(ext, p.interiors().map(|r| coords(&r)).filter(|r| !r.0.is_empty()).collect()))
    }
    fn point(p: &impl PointTrait<T = f64>) -> Option<Point> {
        let c = p.coord()?;
        if c.x().is_nan() && c.y().is_nan() {
            return None;
        }
        Some(Point::new(c.x(), c.y()))
    }
    match w.as_type() {
        GeometryType::Point(p) => point(p).map(Geometry::Point),
        GeometryType::LineString(l) => {
            let l = coords(l);
            (!l.0.is_empty()).then_some(Geometry::LineString(l))
        }
        GeometryType::Polygon(p) => poly(p).map(Geometry::Polygon),
        GeometryType::MultiPoint(m) => {
            let v: Vec<Point> = m.points().filter_map(|p| point(&p)).collect();
            (!v.is_empty()).then(|| Geometry::MultiPoint(MultiPoint::new(v)))
        }
        GeometryType::MultiLineString(m) => {
            let v: Vec<LineString> = m.line_strings().map(|l| coords(&l)).filter(|l| !l.0.is_empty()).collect();
            (!v.is_empty()).then(|| Geometry::MultiLineString(MultiLineString::new(v)))
        }
        GeometryType::MultiPolygon(m) => {
            let v: Vec<Polygon> = m.polygons().filter_map(|p| poly(&p)).collect();
            (!v.is_empty()).then(|| Geometry::MultiPolygon(MultiPolygon::new(v)))
        }
        GeometryType::GeometryCollection(gc) => {
            let v: Vec<Geometry> = gc.geometries().filter_map(|g| to_geo(&g)).collect();
            (!v.is_empty()).then(|| Geometry::GeometryCollection(GeometryCollection::new_from(v)))
        }
        _ => None,
    }
}

fn polygons(g: &Geometry) -> Vec<&Polygon> {
    match g {
        Geometry::Polygon(p) => vec![p],
        Geometry::MultiPolygon(m) => m.0.iter().collect(),
        Geometry::GeometryCollection(gc) => gc.0.iter().flat_map(polygons).collect(),
        _ => vec![],
    }
}

/// G2 bug 11: a ring with zero area is invalid in PostGIS.
fn has_zero_area_ring(g: &Geometry) -> bool {
    polygons(g).iter().any(|p| {
        std::iter::once(p.exterior())
            .chain(p.interiors())
            .any(|r| Polygon::new(r.clone(), vec![]).unsigned_area() == 0.0)
    })
}

fn linear_length(g: &Geometry) -> f64 {
    match g {
        Geometry::Line(l) => Euclidean.length(l),
        Geometry::LineString(l) => Euclidean.length(l),
        Geometry::MultiLineString(l) => Euclidean.length(l),
        Geometry::GeometryCollection(gc) => gc.0.iter().map(linear_length).sum(),
        _ => 0.0,
    }
}

fn yx_less(a: &Coord, b: &Coord) -> bool {
    (a.y, a.x) < (b.y, b.x)
}

/// Maps `geo`'s hull polygon to the PostGIS (JTS) form: a POINT for one distinct input point, a
/// LINESTRING in input order for two, a LINESTRING from the lowest-Y (then lowest-X) to the
/// highest point for collinear input, otherwise a clockwise ring starting at the lowest-Y (then
/// lowest-X) vertex.
pub fn hull_normalize(g: &Geometry, p: &Polygon) -> Geometry {
    use geo::CoordsIter;
    let mut unique: Vec<Coord> = Vec::new();
    for c in g.coords_iter() {
        if !unique.contains(&c) {
            unique.push(c);
            if unique.len() > 2 {
                break;
            }
        }
    }
    match unique.len() {
        0 => return Geometry::GeometryCollection(GeometryCollection::new_from(vec![])),
        1 => return Point::from(unique[0]).into(),
        2 => return LineString::new(unique).into(),
        _ => {}
    }
    let mut cs: Vec<Coord> = p.exterior().0.clone();
    if cs.len() > 1 && cs.first() == cs.last() {
        cs.pop();
    }
    let mut distinct = cs.clone();
    distinct.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap());
    distinct.dedup();
    if distinct.len() == 1 {
        return Point::from(distinct[0]).into();
    }
    if Polygon::new(LineString::new(cs.clone()), vec![]).unsigned_area() == 0.0 {
        return LineString::new(vec![distinct[0], *distinct.last().unwrap()]).into();
    }
    ring_cw_from_lowest(remove_collinear(cs)).into()
}

/// Drops vertices that are exactly collinear with their neighbours (robust orientation test),
/// repeatedly, as JTS's hull does. `geo`'s quickhull keeps some, and can retrace an edge.
fn remove_collinear(mut cs: Vec<Coord>) -> Vec<Coord> {
    use geo::kernels::{Kernel, Orientation};
    loop {
        let n = cs.len();
        if n <= 3 {
            return cs;
        }
        let Some(i) = (0..n).find(|&i| {
            let (a, b, c) = (cs[(i + n - 1) % n], cs[i], cs[(i + 1) % n]);
            a == b || <f64 as geo::GeoNum>::Ker::orient2d(a, b, c) == Orientation::Collinear
        }) else {
            return cs;
        };
        cs.remove(i);
    }
}

fn ring_cw_from_lowest(mut cs: Vec<Coord>) -> Polygon {
    // Clockwise: negative signed area.
    let ring = LineString::new(cs.iter().copied().chain(std::iter::once(cs[0])).collect());
    if Polygon::new(ring, vec![]).signed_area() > 0.0 {
        cs.reverse();
    }
    let start = (0..cs.len()).fold(0, |m, i| if yx_less(&cs[i], &cs[m]) { i } else { m });
    cs.rotate_left(start);
    cs.push(cs[0]);
    Polygon::new(LineString::new(cs), vec![])
}

/// ST_OrientedEnvelope: a POINT for a point hull, a LINESTRING between the lowest-X (then
/// lowest-Y) and highest points for a collinear hull, otherwise `geo`'s rectangle made clockwise
/// and started at the corner where the side containing the first (in JTS hull order) collinear
/// hull edge begins.
pub fn envelope_normalize(g: &Geometry) -> Option<Geometry> {
    let hull_poly = g.convex_hull();
    let hull = hull_normalize(g, &hull_poly);
    match &hull {
        Geometry::Point(_) => return Some(hull),
        Geometry::LineString(l) => {
            let mut v = l.0.clone();
            v.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap());
            return Some(LineString::new(vec![v[0], *v.last().unwrap()]).into());
        }
        _ => {}
    }
    let r = g.minimum_rotated_rect()?;
    let mut cs = r.exterior().0.clone();
    cs.pop();
    let ring = ring_cw_from_lowest(cs);
    let mut cs = ring.exterior().0.clone();
    cs.pop();
    // GEOS (JTS MinimumAreaRectangle) iterates the hull edges in JTS hull order (clockwise from
    // the lowest-Y, lowest-X vertex) and builds the ring from the first edge giving the minimum
    // area, starting at the corner where the side through that edge begins.
    let Geometry::Polygon(h) = &hull else { unreachable!() };
    let mut hv = h.exterior().0.clone();
    hv.pop();
    let scale = cs.iter().chain(hv.iter()).fold(0.0f64, |m, c| m.max(c.x.abs()).max(c.y.abs()));
    let eps = 1e-9 * scale.max(f64::MIN_POSITIVE);
    let on_line = |p: &Coord, a: &Coord, b: &Coord| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return false;
        }
        ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len <= eps
    };
    let n = cs.len();
    'outer: for j in 0..hv.len() {
        let (a, b) = (hv[j], hv[(j + 1) % hv.len()]);
        for i in 0..n {
            let (c0, c1) = (cs[i], cs[(i + 1) % n]);
            if on_line(&a, &c0, &c1) && on_line(&b, &c0, &c1) {
                cs.rotate_left(i);
                break 'outer;
            }
        }
    }
    cs.push(cs[0]);
    Some(Polygon::new(LineString::new(cs), vec![]).into())
}

/// ST_Simplify: Douglas-Peucker per line/ring, then PostGIS's collapse rules: lines keep at
/// least 2 points, rings with fewer than 4 points are dropped, a polygon whose shell collapsed is
/// dropped, and an empty result is NULL.
fn simplify(g: &Geometry, tol: f64, vw: bool) -> Option<Geometry> {
    // PostGIS's DP drops points at distance <= tol even for tol = 0 (`geo` returns the input for
    // epsilon <= 0); its VW drops points with area < tol (`geo`: area <= epsilon).
    let line = |l: &LineString| -> LineString {
        if vw { l.simplify_vw(tol.next_down()) } else { l.simplify(if tol == 0.0 { f64::MIN_POSITIVE } else { tol }) }
    };
    let poly = |p: &Polygon| -> Option<Polygon> {
        let ext = line(p.exterior());
        if ext.0.len() < 4 {
            return None;
        }
        Some(Polygon::new(ext, p.interiors().iter().map(&line).filter(|r| r.0.len() >= 4).collect()))
    };
    // DP drops a line that collapsed to two identical points; VW keeps it.
    let ls = |l: &LineString| -> Option<LineString> {
        let s = line(l);
        (s.0.len() >= 2 && (vw || l.0.len() <= 2 || s.0.len() > 2 || s.0[0] != s.0[1])).then_some(s)
    };
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => Some(g.clone()),
        Geometry::LineString(l) => ls(l).map(Geometry::LineString),
        Geometry::Polygon(p) => poly(p).map(Geometry::Polygon),
        Geometry::MultiLineString(m) => {
            let v: Vec<_> = m.0.iter().filter_map(ls).collect();
            (!v.is_empty()).then(|| MultiLineString::new(v).into())
        }
        Geometry::MultiPolygon(m) => {
            let v: Vec<_> = m.0.iter().filter_map(poly).collect();
            (!v.is_empty()).then(|| MultiPolygon::new(v).into())
        }
        Geometry::GeometryCollection(gc) => {
            let v: Vec<_> = gc.0.iter().filter_map(|c| simplify(c, tol, vw)).collect();
            (!v.is_empty()).then(|| Geometry::GeometryCollection(GeometryCollection::new_from(v)))
        }
        _ => Some(g.clone()),
    }
}

pub fn unary(func: &str, g: Option<&Geometry>, input: &str, tol: f64, vwtol: f64) -> String {
    let Some(g) = g else {
        // EMPTY rules (PostGIS behaviour, G2 §7).
        return match func {
            "st_isvalid" => "true".into(),
            "st_pointonsurface" | "st_centroid" => "POINT EMPTY".into(),
            "st_convexhull" | "st_simplify" | "st_simplifyvw" => input.into(),
            "st_orientedenvelope" => "POLYGON EMPTY".into(),
            "st_area" | "st_length" => "0".into(),
            _ => NA.into(),
        };
    };
    match func {
        "st_isvalid" => (g.is_valid() && !has_zero_area_ring(g)).to_string(),
        "st_pointonsurface" => g.interior_point().map_or("POINT EMPTY".into(), |p| render_geo(&p.into())),
        "st_convexhull" => render_geo(&hull_normalize(g, &g.convex_hull())),
        "st_orientedenvelope" => envelope_normalize(g).map_or("NULL".into(), |e| render_geo(&e)),
        "st_simplify" | "st_simplifyvw" => {
            use geo::CoordsIter;
            let vw = func == "st_simplifyvw";
            match simplify(g, if vw { vwtol } else { tol }, vw) {
                None => "NULL".into(),
                // PostGIS returns the input unchanged (EMPTY parts included) when no point was removed.
                Some(s) if s.coords_count() == g.coords_count() => input.into(),
                Some(s) => render_geo(&s),
            }
        }
        "st_centroid" => g.centroid().map_or("POINT EMPTY".into(), |p| render_geo(&p.into())),
        "st_area" => crate::f(g.unsigned_area()),
        "st_length" => crate::f(linear_length(g)),
        _ => unreachable!(),
    }
}

#[allow(dead_code)]
fn distance(a: &Geometry, b: &Geometry) -> f64 {
    Euclidean.distance(a, b)
}
