//! The bridge between geo-traits geometries and GEOS, used by every GEOS-backed UDF.

mod column;
mod convert;

pub(crate) use column::GeosColumn;
pub(crate) use convert::{empty_like, from_geos, to_geos};
