use geodatafusion::udf::native::io::{
    AsBinary, AsEWKT, AsText, GeomFromEWKT, GeomFromText, GeomFromWKB,
};

use crate::{impl_udf, impl_udf_coord_type_arg};

impl_udf!(AsBinary, PyAsBinary, "AsBinary");
impl_udf!(AsEWKT, PyAsEWKT, "AsEWKT");
impl_udf!(AsText, PyAsText, "AsText");
impl_udf!(GeomFromEWKT, PyGeomFromEWKT, "GeomFromEWKT");

impl_udf_coord_type_arg!(GeomFromWKB, PyGeomFromWKB, "GeomFromWKB");
impl_udf_coord_type_arg!(GeomFromText, PyGeomFromText, "GeomFromText");
