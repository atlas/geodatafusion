//! The geometry of a box, as PostGIS's box2d and box3d casts to geometry build it.

use datafusion::common::not_impl_err;
use datafusion::error::Result;
use wkt::Wkt;
use wkt::types::{Coord, LineString, Point, Polygon};

/// The geometry covering the box from `min` to `max`, which have two (XY) or three (XYZ)
/// ordinates each.
///
/// As in PostGIS, a box that is flat in every dimension is a point, one flat in all but one is a
/// line from `min` to `max`, and one flat in a single dimension is a polygon in that plane. A 3D
/// box flat in none is a POLYHEDRALSURFACE in PostGIS, which GeoArrow can't hold.
pub(crate) fn box_geometry(min: &[f64], max: &[f64]) -> Result<Wkt<f64>> {
    let coord = |ordinates: [f64; 3]| Coord {
        x: ordinates[0],
        y: ordinates[1],
        z: (min.len() > 2).then_some(ordinates[2]),
        m: None,
    };
    let lo = [min[0], min[1], min.get(2).copied().unwrap_or_default()];
    let hi = [max[0], max[1], max.get(2).copied().unwrap_or_default()];
    let flat: Vec<bool> = (0..min.len()).map(|axis| lo[axis] == hi[axis]).collect();
    let flat_count = flat.iter().filter(|flat| **flat).count();
    if flat_count == min.len() {
        return Ok(Wkt::Point(Point::from_coord(coord(lo))));
    }
    if flat_count + 1 == min.len() {
        let line = LineString::from_coords([coord(lo), coord(hi)])
            .expect("two coordinates of one dimension make a line");
        return Ok(Wkt::LineString(line));
    }
    if flat_count == 0 && min.len() > 2 {
        return not_impl_err!("A 3D box with volume is a POLYHEDRALSURFACE, which isn't supported");
    }
    // The ring's corners as (low or high) per axis, in PostGIS's order for each flat axis.
    let corners: [[bool; 3]; 4] = match flat.iter().position(|flat| *flat) {
        Some(0) => [
            [false, false, false],
            [false, true, false],
            [false, true, true],
            [false, false, true],
        ],
        Some(1) => [
            [false, false, false],
            [true, false, false],
            [true, false, true],
            [false, false, true],
        ],
        _ => [
            [false, false, false],
            [false, true, false],
            [true, true, false],
            [true, false, false],
        ],
    };
    let corner = |high: [bool; 3]| {
        coord(std::array::from_fn(|axis| {
            if high[axis] { hi[axis] } else { lo[axis] }
        }))
    };
    let ring = LineString::from_coords(corners.iter().chain(&corners[..1]).map(|c| corner(*c)))
        .expect("five coordinates of one dimension make a ring");
    Ok(Wkt::Polygon(
        Polygon::from_rings([ring]).expect("one ring makes a polygon"),
    ))
}
