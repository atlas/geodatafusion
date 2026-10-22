//! Polygon clipping and validation with [Wagyu](https://github.com/mapbox/wagyu), the library
//! PostGIS uses in ST_AsMVTGeom.
//!
//! Wagyu 0.5.0 and the parts of [geometry.hpp](https://github.com/mapbox/geometry.hpp) 1.0.0 it
//! needs are vendored in `vendor/` (ISC licence, see `vendor/LICENSE.*`) and compiled by
//! `build.rs`. They are the versions PostGIS 3.6 bundles, unchanged.

use std::ffi::{CStr, c_char, c_int, c_void};

/// A point in Wagyu's integer coordinates.
pub type Point = [i32; 2];

/// A closed ring.
pub type Ring = Vec<Point>;

/// A polygon: its shell, then its holes.
pub type Polygon = Vec<Ring>;

/// The box to clip to, edges included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

/// Which areas count as inside, by the winding number of the rings around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillType {
    /// An odd winding number.
    EvenOdd,
    /// A winding number other than 0.
    NonZero,
    /// A positive winding number.
    Positive,
    /// A negative winding number.
    Negative,
}

impl FillType {
    /// Wagyu's `fill_type` enumerator.
    fn to_c(self) -> c_int {
        match self {
            FillType::EvenOdd => 0,
            FillType::NonZero => 1,
            FillType::Positive => 2,
            FillType::Negative => 3,
        }
    }
}

/// An exception Wagyu threw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wagyu: {}", self.0)
    }
}

impl std::error::Error for Error {}

type PointFn = unsafe extern "C" fn(*mut c_void, i32, i32);
type EndFn = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" {
    fn gdf_wagyu_clip(
        coords: *const i32,
        ring_ends: *const usize,
        num_rings: usize,
        min_x: i32,
        min_y: i32,
        max_x: i32,
        max_y: i32,
        fill_type: c_int,
        context: *mut c_void,
        on_point: PointFn,
        on_ring_end: EndFn,
        on_polygon_end: EndFn,
        error: *mut c_char,
        error_len: usize,
    ) -> c_int;
}

/// Clips the rings to `bounds` and unions them under `fill`, giving valid polygons with their
/// shells clockwise and their holes counter-clockwise in a y-up system.
///
/// The rings are clipped one by one first (Wagyu's `quick_clip`, which rounds intersections to
/// the nearest integer), and then all of them make up one subject, so which polygon a ring came
/// from doesn't matter, only the winding numbers.
pub fn clip(rings: &[Ring], bounds: Bounds, fill: FillType) -> Result<Vec<Polygon>, Error> {
    let coords: Vec<i32> = rings.iter().flatten().flatten().copied().collect();
    let ring_ends: Vec<usize> = rings
        .iter()
        .scan(0, |end, ring| {
            *end += ring.len();
            Some(*end)
        })
        .collect();
    let mut builder = Builder::default();
    let mut error = [0 as c_char; 256];
    // SAFETY: `coords` holds the x, y pairs of every ring, `ring_ends` has one cumulative end
    // per ring, and the callbacks only touch the `Builder` passed as the context, which lives
    // until the call returns. The error buffer is written NUL-terminated within its length.
    let status = unsafe {
        gdf_wagyu_clip(
            coords.as_ptr(),
            ring_ends.as_ptr(),
            ring_ends.len(),
            bounds.min_x,
            bounds.min_y,
            bounds.max_x,
            bounds.max_y,
            fill.to_c(),
            (&mut builder as *mut Builder).cast(),
            on_point,
            on_ring_end,
            on_polygon_end,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status != 0 {
        // SAFETY: gdf_wagyu_clip wrote a NUL-terminated message into the buffer.
        let message = unsafe { CStr::from_ptr(error.as_ptr()) };
        return Err(Error(message.to_string_lossy().into_owned()));
    }
    Ok(builder.polygons)
}

/// The result, assembled from the callbacks.
#[derive(Default)]
struct Builder {
    polygons: Vec<Polygon>,
    polygon: Polygon,
    ring: Ring,
}

unsafe extern "C" fn on_point(context: *mut c_void, x: i32, y: i32) {
    // SAFETY: the context is the `Builder` passed to gdf_wagyu_clip.
    let builder = unsafe { &mut *context.cast::<Builder>() };
    builder.ring.push([x, y]);
}

unsafe extern "C" fn on_ring_end(context: *mut c_void) {
    // SAFETY: as in on_point.
    let builder = unsafe { &mut *context.cast::<Builder>() };
    let ring = std::mem::take(&mut builder.ring);
    builder.polygon.push(ring);
}

unsafe extern "C" fn on_polygon_end(context: *mut c_void) {
    // SAFETY: as in on_point.
    let builder = unsafe { &mut *context.cast::<Builder>() };
    let polygon = std::mem::take(&mut builder.polygon);
    builder.polygons.push(polygon);
}

#[cfg(test)]
mod test {
    use super::*;

    const BOUNDS: Bounds = Bounds {
        min_x: 0,
        min_y: 0,
        max_x: 10,
        max_y: 10,
    };

    #[test]
    fn test_clip_to_bounds() {
        let square = vec![[-5, -5], [15, -5], [15, 15], [-5, 15], [-5, -5]];
        let result = clip(&[square], BOUNDS, FillType::EvenOdd).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        let mut corners = result[0][0].clone();
        corners.sort();
        corners.dedup();
        assert_eq!(corners, vec![[0, 0], [0, 10], [10, 0], [10, 10]]);
    }

    #[test]
    fn test_even_odd_cancels_overlap() {
        let square = vec![[1, 1], [3, 1], [3, 3], [1, 3], [1, 1]];
        let result = clip(&[square.clone(), square], BOUNDS, FillType::EvenOdd).unwrap();
        assert!(result.is_empty());
    }
}
