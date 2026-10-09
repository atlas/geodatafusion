use geodatafusion::udf::native::io::{
    AsBinary, AsEWKB, AsEWKT, AsGeoJSON, AsHEXEWKB, AsText, Box2DFromGeoHash, GeoHash,
    GeomCollFromText, GeomCollFromWKB, GeomFromEWKB, GeomFromEWKT, GeomFromGeoHash,
    GeomFromGeoJSON, GeomFromText, GeomFromWKB, LineFromText, LineFromWKB, MLineFromText,
    MLineFromWKB, MPointFromText, MPointFromWKB, MPolyFromText, MPolyFromWKB, PointFromGeoHash,
    PointFromText, PointFromWKB, PolyFromWKB, PolygonFromText,
};

use crate::impl_udf;

impl_udf!(AsBinary, PyAsBinary, "AsBinary");
impl_udf!(AsEWKB, PyAsEWKB, "AsEWKB");
impl_udf!(AsHEXEWKB, PyAsHEXEWKB, "AsHEXEWKB");
impl_udf!(AsEWKT, PyAsEWKT, "AsEWKT");
impl_udf!(AsText, PyAsText, "AsText");
impl_udf!(GeomFromEWKB, PyGeomFromEWKB, "GeomFromEWKB");
impl_udf!(GeomFromEWKT, PyGeomFromEWKT, "GeomFromEWKT");

impl_udf!(GeomFromWKB, PyGeomFromWKB, "GeomFromWKB");
impl_udf!(GeomFromText, PyGeomFromText, "GeomFromText");
impl_udf!(GeoHash, PyGeoHash, "GeoHash");
impl_udf!(PointFromGeoHash, PyPointFromGeoHash, "PointFromGeoHash");
impl_udf!(GeomFromGeoHash, PyGeomFromGeoHash, "GeomFromGeoHash");
impl_udf!(Box2DFromGeoHash, PyBox2DFromGeoHash, "Box2DFromGeoHash");
impl_udf!(PointFromText, PyPointFromText, "PointFromText");
impl_udf!(LineFromText, PyLineFromText, "LineFromText");
impl_udf!(PolygonFromText, PyPolygonFromText, "PolygonFromText");
impl_udf!(MPointFromText, PyMPointFromText, "MPointFromText");
impl_udf!(MLineFromText, PyMLineFromText, "MLineFromText");
impl_udf!(MPolyFromText, PyMPolyFromText, "MPolyFromText");
impl_udf!(GeomCollFromText, PyGeomCollFromText, "GeomCollFromText");
impl_udf!(PointFromWKB, PyPointFromWKB, "PointFromWKB");
impl_udf!(LineFromWKB, PyLineFromWKB, "LineFromWKB");
impl_udf!(PolyFromWKB, PyPolyFromWKB, "PolyFromWKB");
impl_udf!(MPointFromWKB, PyMPointFromWKB, "MPointFromWKB");
impl_udf!(MLineFromWKB, PyMLineFromWKB, "MLineFromWKB");
impl_udf!(MPolyFromWKB, PyMPolyFromWKB, "MPolyFromWKB");
impl_udf!(GeomCollFromWKB, PyGeomCollFromWKB, "GeomCollFromWKB");
impl_udf!(AsGeoJSON, PyAsGeoJSON, "AsGeoJSON");
impl_udf!(GeomFromGeoJSON, PyGeomFromGeoJSON, "GeomFromGeoJSON");
