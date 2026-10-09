//! PostGIS's single-precision bounding boxes.

use crate::udf::native::bounding_box::util::bounds::BoundingRect;

/// A bounding box as PostGIS stores it in a geometry: each range rounded outward to single
/// precision. Z and M are there when the geometry has them. The operators compare these boxes,
/// and ST_BoundingDiagonal returns one by default.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FloatBox {
    pub(crate) x: (f64, f64),
    pub(crate) y: (f64, f64),
    pub(crate) z: Option<(f64, f64)>,
    pub(crate) m: Option<(f64, f64)>,
}

impl FloatBox {
    /// The box of `rect`, or `None` for an EMPTY geometry.
    pub(crate) fn new(rect: &BoundingRect) -> Option<Self> {
        if rect.is_empty() {
            return None;
        }
        let round = |(min, max): (f64, f64)| (round_down(min), round_up(max));
        Some(Self {
            x: round((rect.minx(), rect.maxx())),
            y: round((rect.miny(), rect.maxy())),
            z: rect.z_range().map(round),
            m: rect.m_range().map(round),
        })
    }
}

/// The largest single-precision float not above `value`.
fn round_down(value: f64) -> f64 {
    let rounded = value as f32;
    if rounded as f64 <= value {
        rounded as f64
    } else {
        rounded.next_down() as f64
    }
}

/// The smallest single-precision float not below `value`.
fn round_up(value: f64) -> f64 {
    let rounded = value as f32;
    if rounded as f64 >= value {
        rounded as f64
    } else {
        rounded.next_up() as f64
    }
}
