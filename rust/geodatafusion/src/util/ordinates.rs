//! Named access to the Z and M ordinates of a coordinate.
//!
//! `CoordTrait::nth(2)` is Z for an XYZ coordinate but M for an XYM one; read Z and M with these
//! helpers instead.

use geo_traits::{CoordTrait, Dimensions};

/// The Z ordinate, if the coordinate has one.
pub(crate) fn z(coord: &impl CoordTrait<T = f64>) -> Option<f64> {
    match coord.dim() {
        Dimensions::Xyz | Dimensions::Xyzm => coord.nth(2),
        _ => None,
    }
}

/// The M ordinate, if the coordinate has one.
pub(crate) fn m(coord: &impl CoordTrait<T = f64>) -> Option<f64> {
    match coord.dim() {
        Dimensions::Xym => coord.nth(2),
        Dimensions::Xyzm => coord.nth(3),
        _ => None,
    }
}
