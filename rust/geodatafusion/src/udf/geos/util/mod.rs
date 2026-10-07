//! The bridge between geo-traits geometries and GEOS, used by every GEOS-backed UDF.

mod convert;

pub(crate) use convert::{empty_like, from_geos, to_geos};
