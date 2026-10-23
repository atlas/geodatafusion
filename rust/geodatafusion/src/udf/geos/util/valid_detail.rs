//! GEOS's `GEOSisValidDetail_r`, which the geos crate doesn't bind and whose geometries it
//! doesn't let us pass to `geos::sys`. So this calls it with its own GEOS context, on a geometry
//! read from WKB, as the PROJ wrapper does for PROJ.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use geo_traits::GeometryTrait;
use geos::sys::{
    GEOS_finish_r, GEOS_init_r, GEOSContextHandle_t, GEOSFree_r, GEOSGeom_destroy_r,
    GEOSGeomGetX_r, GEOSGeomGetY_r, GEOSGeometry, GEOSWKBReader_create_r, GEOSWKBReader_destroy_r,
    GEOSWKBReader_read_r, GEOSisValidDetail_r,
};

use crate::udf::native::io::util::wkb::write_wkb;

/// Whether a geometry is valid, and if not, why and where.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidDetail {
    pub(crate) valid: bool,
    pub(crate) reason: Option<String>,
    /// The location of the problem, as X and Y.
    pub(crate) location: Option<(f64, f64)>,
}

/// A GEOS context of our own, one per thread.
struct Context(GEOSContextHandle_t);

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the handle came from GEOS_init_r and is finished once.
        unsafe { GEOS_finish_r(self.0) }
    }
}

thread_local! {
    // SAFETY: GEOS_init_r has no preconditions.
    static CONTEXT: Context = Context(unsafe { GEOS_init_r() });
}

/// The validity of a geometry by GEOS's rules, with `flags` (1 allows self-touching rings that
/// form holes, as ESRI does), or GEOS's failure.
pub(crate) fn valid_detail(
    geom: &impl GeometryTrait<T = f64>,
    flags: i32,
) -> Result<ValidDetail, &'static str> {
    let mut wkb = Vec::new();
    write_wkb(&mut wkb, geom, wkb::Endianness::LittleEndian)
        .map_err(|_| "couldn't write the geometry as WKB")?;
    CONTEXT.with(|context| {
        let handle = context.0;
        // SAFETY: every GEOS object made here is destroyed here, with the handle it was made
        // with, and the WKB outlives the read.
        unsafe {
            let reader = GEOSWKBReader_create_r(handle);
            if reader.is_null() {
                return Err("GEOS couldn't create a WKB reader");
            }
            let geometry = GEOSWKBReader_read_r(handle, reader, wkb.as_ptr(), wkb.len());
            GEOSWKBReader_destroy_r(handle, reader);
            if geometry.is_null() {
                return Err("GEOS couldn't read the geometry");
            }
            let mut reason: *mut c_char = ptr::null_mut();
            let mut location: *mut GEOSGeometry = ptr::null_mut();
            let result = GEOSisValidDetail_r(handle, geometry, flags, &mut reason, &mut location);
            GEOSGeom_destroy_r(handle, geometry);
            let detail = match result {
                1 => Ok(ValidDetail {
                    valid: true,
                    reason: None,
                    location: None,
                }),
                0 => Ok(ValidDetail {
                    valid: false,
                    reason: (!reason.is_null())
                        .then(|| CStr::from_ptr(reason).to_string_lossy().into_owned()),
                    location: point(handle, location),
                }),
                _ => Err("GEOSisValidDetail failed"),
            };
            if !reason.is_null() {
                GEOSFree_r(handle, reason.cast::<c_void>());
            }
            if !location.is_null() {
                GEOSGeom_destroy_r(handle, location);
            }
            detail
        }
    })
}

/// The X and Y of a GEOS point, if it is one.
///
/// # Safety
///
/// `location` is null or a geometry made with `handle`.
unsafe fn point(handle: GEOSContextHandle_t, location: *const GEOSGeometry) -> Option<(f64, f64)> {
    if location.is_null() {
        return None;
    }
    let (mut x, mut y) = (0.0, 0.0);
    // SAFETY: the caller's.
    let read = unsafe {
        GEOSGeomGetX_r(handle, location, &mut x) == 1
            && GEOSGeomGetY_r(handle, location, &mut y) == 1
    };
    read.then_some((x, y))
}
