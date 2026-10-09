//! PROJ coordinate operations, created and applied the way PostGIS does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::rc::Rc;

use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use proj_sys::{
    PJ, PJ_CONTEXT, PJ_COORD, PJ_DIRECTION_PJ_FWD, PJ_DIRECTION_PJ_INV, PJ_LOG_LEVEL_PJ_LOG_NONE,
    PJ_XYZT, proj_angular_input, proj_angular_output, proj_context_create, proj_context_destroy,
    proj_context_errno, proj_context_errno_string, proj_create, proj_create_crs_to_crs,
    proj_destroy, proj_errno, proj_errno_reset, proj_errno_string, proj_log_func, proj_log_level,
    proj_normalize_for_visualization, proj_trans,
};
use wkt::types::Coord;

/// How a coordinate operation is defined.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Definition {
    /// From one CRS to another, each anything `proj_create` reads (`EPSG:4326`, a PROJ string,
    /// WKT, PROJJSON).
    CrsToCrs { from: String, to: String },
    /// A pipeline or coordinate operation (`urn:ogc:def:coordinateOperation:EPSG::16031`, a PROJ
    /// pipeline string), run forward or inverse.
    Pipeline { definition: String, inverse: bool },
}

/// A PROJ coordinate operation with its own context. PROJ objects aren't thread-safe, so
/// operations live in a per-thread cache (see [`with_operation`]).
pub(crate) struct Operation {
    context: *mut PJ_CONTEXT,
    pj: *mut PJ,
    inverse: bool,
}

impl Operation {
    /// Creates the operation as PostGIS does: `proj_create_crs_to_crs` (or `proj_create` for a
    /// pipeline), then `proj_normalize_for_visualization` for longitude/latitude order. An
    /// operation that can't be normalised (a bare conversion such as EPSG::16031) is used as it
    /// is, which is what PostGIS's results show it doing.
    fn new(name: &str, definition: &Definition) -> Result<Self> {
        let cstring = |text: &str| {
            CString::new(text)
                .map_err(|_| exec_datafusion_err!("{name}: a CRS can't contain a NUL character"))
        };
        // SAFETY: the context and objects are created, used and destroyed here and in `Drop`
        // only, on one thread; the strings outlive the calls that read them.
        unsafe {
            let context = proj_context_create();
            // PROJ's messages go to stderr by default; errors reach the caller instead.
            proj_log_level(context, PJ_LOG_LEVEL_PJ_LOG_NONE);
            proj_log_func(context, std::ptr::null_mut(), Some(discard_log));
            let (pj, inverse) = match definition {
                Definition::CrsToCrs { from, to } => {
                    let (from, to) = (cstring(from)?, cstring(to)?);
                    let pj = proj_create_crs_to_crs(
                        context,
                        from.as_ptr(),
                        to.as_ptr(),
                        std::ptr::null_mut(),
                    );
                    (pj, false)
                }
                Definition::Pipeline {
                    definition,
                    inverse,
                } => {
                    let definition = cstring(definition)?;
                    (proj_create(context, definition.as_ptr()), *inverse)
                }
            };
            if pj.is_null() {
                let message = context_error(context);
                proj_context_destroy(context);
                return Err(exec_datafusion_err!(
                    "{name}: could not form projection: {message}"
                ));
            }
            let normalized = proj_normalize_for_visualization(context, pj);
            let pj = if normalized.is_null() {
                proj_errno_reset(pj);
                pj
            } else {
                proj_destroy(pj);
                normalized
            };
            Ok(Self {
                context,
                pj,
                inverse,
            })
        }
    }

    /// Transforms a coordinate. Like PostGIS, angles go in and out in degrees, a 2D coordinate
    /// is transformed with Z 0 (and stays 2D), and M is kept.
    pub(crate) fn transform(&self, name: &str, coord: Coord<f64>) -> Result<Coord<f64>> {
        let direction = if self.inverse {
            PJ_DIRECTION_PJ_INV
        } else {
            PJ_DIRECTION_PJ_FWD
        };
        // SAFETY: `self.pj` is a live operation created on this thread.
        unsafe {
            proj_errno_reset(self.pj);
            let (x, y) = if proj_angular_input(self.pj, direction) != 0 {
                (coord.x.to_radians(), coord.y.to_radians())
            } else {
                (coord.x, coord.y)
            };
            let input = PJ_COORD {
                xyzt: PJ_XYZT {
                    x,
                    y,
                    z: coord.z.unwrap_or(0.0),
                    t: f64::INFINITY,
                },
            };
            let output = proj_trans(self.pj, direction, input).xyzt;
            let errno = proj_errno(self.pj);
            if errno != 0 {
                let message = CStr::from_ptr(proj_errno_string(errno)).to_string_lossy();
                return Err(exec_datafusion_err!(
                    "{name}: transform: {message} ({errno})"
                ));
            }
            let (x, y) = if proj_angular_output(self.pj, direction) != 0 {
                (output.x.to_degrees(), output.y.to_degrees())
            } else {
                (output.x, output.y)
            };
            Ok(Coord {
                x,
                y,
                z: coord.z.map(|_| output.z),
                m: coord.m,
            })
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        // SAFETY: both were created by `Operation::new` and are destroyed once.
        unsafe {
            proj_destroy(self.pj);
            proj_context_destroy(self.context);
        }
    }
}

/// A PROJ logger that drops every message.
unsafe extern "C" fn discard_log(_: *mut c_void, _: c_int, _: *const c_char) {}

/// The message of the context's last error.
///
/// # Safety
///
/// `context` must be a live context.
unsafe fn context_error(context: *mut PJ_CONTEXT) -> String {
    // SAFETY: the caller passes a live context; PROJ returns a static or context-owned string.
    unsafe {
        let errno = proj_context_errno(context);
        let message = proj_context_errno_string(context, errno);
        if message.is_null() {
            format!("error {errno}")
        } else {
            CStr::from_ptr(message).to_string_lossy().into_owned()
        }
    }
}

thread_local! {
    /// Operations by definition: creating one looks it up in `proj.db`, which takes
    /// milliseconds.
    static OPERATIONS: RefCell<HashMap<Definition, Rc<Operation>>> = RefCell::new(HashMap::new());
}

/// Runs `f` with the operation for `definition`, created once per thread.
pub(crate) fn with_operation<T>(
    name: &str,
    definition: &Definition,
    f: impl FnOnce(&Operation) -> T,
) -> Result<T> {
    let cached = OPERATIONS.with(|operations| operations.borrow().get(definition).cloned());
    let operation = match cached {
        Some(operation) => operation,
        None => {
            let operation = Rc::new(Operation::new(name, definition)?);
            OPERATIONS.with(|operations| {
                operations
                    .borrow_mut()
                    .insert(definition.clone(), Rc::clone(&operation))
            });
            operation
        }
    };
    Ok(f(&operation))
}
