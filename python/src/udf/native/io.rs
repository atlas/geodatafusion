use geodatafusion::udf::native::io::{
    AsBinary, AsEWKT, AsText, Box2DFromGeoHash, GeoHash, GeomFromEWKT, GeomFromGeoHash,
    GeomFromText, GeomFromWKB, PointFromGeoHash,
};

use crate::impl_udf;

impl_udf!(AsBinary, PyAsBinary, "AsBinary");
impl_udf!(AsEWKT, PyAsEWKT, "AsEWKT");
impl_udf!(AsText, PyAsText, "AsText");
impl_udf!(GeomFromEWKT, PyGeomFromEWKT, "GeomFromEWKT");

impl_udf!(GeomFromWKB, PyGeomFromWKB, "GeomFromWKB");
impl_udf!(GeomFromText, PyGeomFromText, "GeomFromText");
impl_udf!(GeoHash, PyGeoHash, "GeoHash");
impl_udf!(PointFromGeoHash, PyPointFromGeoHash, "PointFromGeoHash");
impl_udf!(GeomFromGeoHash, PyGeomFromGeoHash, "GeomFromGeoHash");
impl_udf!(Box2DFromGeoHash, PyBox2DFromGeoHash, "Box2DFromGeoHash");
