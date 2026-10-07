//! The bridge between geo-traits geometries and GEOS, used by every GEOS-backed UDF.

mod column;
mod convert;
mod params;

pub(crate) use column::GeosColumn;
pub(crate) use convert::{empty_like, from_geos, has_z, to_geos};
pub(crate) use params::{BufferStyle, StyleKeys};
