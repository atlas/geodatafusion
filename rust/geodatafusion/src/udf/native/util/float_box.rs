//! PostGIS's single-precision bounding boxes.

use geo_traits::{
    GeometryTrait, GeometryType, LineStringTrait, MultiLineStringTrait, MultiPointTrait,
};

use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
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

/// Whether PostGIS stores a bounding box with the geometry, as recorded: never when it is
/// empty, for a point or a two-point line, nor for a MULTIPOINT or MULTILINESTRING of one such
/// member; always otherwise.
pub(crate) fn has_stored_box(geom: &impl GeometryTrait<T = f64>) -> bool {
    if is_geometry_topologically_empty(geom) {
        return false;
    }
    match geom.as_type() {
        GeometryType::Point(_) => false,
        GeometryType::LineString(line) => line.num_coords() > 2,
        GeometryType::Line(_) => false,
        GeometryType::MultiPoint(points) => points.num_points() > 1,
        GeometryType::MultiLineString(lines) => {
            lines.num_line_strings() > 1 || lines.line_strings().any(|line| line.num_coords() > 2)
        }
        _ => true,
    }
}

#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_ClipByBox2D, backed by GEOS, reads it so far"
    )
)]
/// The 2D box `[min_x, min_y, max_x, max_y]` PostGIS reads from a stored geometry: the
/// single-precision one stored with it, or the exact one of a geometry stored without (see
/// [`has_stored_box`]). `None` for an EMPTY geometry.
pub(crate) fn serialized_box(geom: &impl GeometryTrait<T = f64>) -> Option<[f64; 4]> {
    let mut rect = BoundingRect::new(false);
    rect.add_geometry(geom);
    if rect.is_empty() {
        return None;
    }
    if !has_stored_box(geom) {
        return Some([rect.minx(), rect.miny(), rect.maxx(), rect.maxy()]);
    }
    let float = FloatBox::new(&rect)?;
    Some([float.x.0, float.y.0, float.x.1, float.y.1])
}

/// The largest single-precision float not above `value`.
pub(crate) fn round_down(value: f64) -> f64 {
    let rounded = value as f32;
    if rounded as f64 <= value {
        rounded as f64
    } else {
        rounded.next_down() as f64
    }
}

/// The smallest single-precision float not below `value`.
pub(crate) fn round_up(value: f64) -> f64 {
    let rounded = value as f32;
    if rounded as f64 >= value {
        rounded as f64
    } else {
        rounded.next_up() as f64
    }
}
