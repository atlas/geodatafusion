use geodatafusion::udf::native::io::{
    AsBinary, AsEWKT, AsText, GeomFromEWKT, GeomFromText, GeomFromWKB,
};

use crate::impl_udf;

impl_udf!(AsBinary, PyAsBinary, "AsBinary");
impl_udf!(AsEWKT, PyAsEWKT, "AsEWKT");
impl_udf!(AsText, PyAsText, "AsText");
impl_udf!(GeomFromEWKT, PyGeomFromEWKT, "GeomFromEWKT");

impl_udf!(GeomFromWKB, PyGeomFromWKB, "GeomFromWKB");
impl_udf!(GeomFromText, PyGeomFromText, "GeomFromText");
