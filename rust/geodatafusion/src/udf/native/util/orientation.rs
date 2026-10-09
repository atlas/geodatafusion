//! The orientation of rings.

use geo_traits::{CoordTrait, LineStringTrait};

/// Twice the signed area of a ring (the shoelace formula): positive when counter-clockwise,
/// negative when clockwise, and zero when it has no area. Coordinates are taken relative to the
/// first one, to lose less precision far from the origin.
pub(crate) fn ring_signed_area(ring: &impl LineStringTrait<T = f64>) -> f64 {
    let Some(origin) = ring.coord(0) else {
        return 0.0;
    };
    let (x0, y0) = (origin.x(), origin.y());
    let coords: Vec<(f64, f64)> = ring.coords().map(|c| (c.x() - x0, c.y() - y0)).collect();
    coords
        .windows(2)
        .map(|pair| pair[0].0 * pair[1].1 - pair[1].0 * pair[0].1)
        .sum()
}
