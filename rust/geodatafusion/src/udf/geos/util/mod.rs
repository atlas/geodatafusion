//! The bridge between geo-traits geometries and GEOS, used by every GEOS-backed UDF.

mod column;
mod convert;
mod params;
mod projected;
mod valid_detail;

pub(crate) use column::GeosColumn;
pub(crate) use convert::{empty_like, empty_of_type, empty_point_like, from_geos, has_z, to_geos};
pub(crate) use params::{BufferStyle, StyleKeys};
pub(crate) use projected::{project, unproject};
pub(crate) use valid_detail::{ValidDetail, valid_detail};
