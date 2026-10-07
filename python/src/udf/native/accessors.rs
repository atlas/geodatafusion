use geodatafusion::udf::native::accessors::{
    CoordDim, EndPoint, GeometryType, IsClosed, M, NDims, NPoints, NumInteriorRings, NumPoints,
    ST_GeometryType, StartPoint, X, Y, Z,
};

use crate::impl_udf;

impl_udf!(CoordDim, PyCoordDim, "CoordDim");
impl_udf!(NDims, PyNDims, "NDims");
impl_udf!(X, PyX, "X");
impl_udf!(Y, PyY, "Y");
impl_udf!(Z, PyZ, "Z");
impl_udf!(M, PyM, "M");
impl_udf!(IsClosed, PyIsClosed, "IsClosed");
impl_udf!(EndPoint, PyEndPoint, "EndPoint");
impl_udf!(StartPoint, PyStartPoint, "StartPoint");
impl_udf!(NPoints, PyNPoints, "NPoints");
impl_udf!(NumPoints, PyNumPoints, "NumPoints");
impl_udf!(NumInteriorRings, PyNumInteriorRings, "NumInteriorRings");
impl_udf!(GeometryType, PyGeometryType, "GeometryType");
impl_udf!(ST_GeometryType, PySTGeometryType, "STGeometryType");
