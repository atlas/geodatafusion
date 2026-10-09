use geodatafusion::udf::native::accessors::{
    BoundingDiagonal, CoordDim, Dimension, Dump, EndPoint, Envelope, ExteriorRing, GeometryN,
    GeometryType, HasM, HasZ, InteriorRingN, IsClosed, IsCollection, IsEmpty, IsPolygonCCW,
    IsPolygonCW, M, NDims, NPoints, NRings, NumGeometries, NumInteriorRings, NumPoints, PointN,
    Points, ST_GeometryType, StartPoint, X, Y, Z, Zmflag,
};

use crate::impl_udf;

impl_udf!(CoordDim, PyCoordDim, "CoordDim");
impl_udf!(NDims, PyNDims, "NDims");
impl_udf!(X, PyX, "X");
impl_udf!(Y, PyY, "Y");
impl_udf!(Z, PyZ, "Z");
impl_udf!(M, PyM, "M");
impl_udf!(IsClosed, PyIsClosed, "IsClosed");
impl_udf!(IsEmpty, PyIsEmpty, "IsEmpty");
impl_udf!(Dump, PyDump, "Dump");
impl_udf!(EndPoint, PyEndPoint, "EndPoint");
impl_udf!(StartPoint, PyStartPoint, "StartPoint");
impl_udf!(NPoints, PyNPoints, "NPoints");
impl_udf!(NumPoints, PyNumPoints, "NumPoints");
impl_udf!(NumInteriorRings, PyNumInteriorRings, "NumInteriorRings");
impl_udf!(GeometryType, PyGeometryType, "GeometryType");
impl_udf!(ST_GeometryType, PySTGeometryType, "STGeometryType");
impl_udf!(Dimension, PyDimension, "Dimension");
impl_udf!(Zmflag, PyZmflag, "Zmflag");
impl_udf!(HasZ, PyHasZ, "HasZ");
impl_udf!(HasM, PyHasM, "HasM");
impl_udf!(IsCollection, PyIsCollection, "IsCollection");
impl_udf!(NumGeometries, PyNumGeometries, "NumGeometries");
impl_udf!(GeometryN, PyGeometryN, "GeometryN");
impl_udf!(NRings, PyNRings, "NRings");
impl_udf!(ExteriorRing, PyExteriorRing, "ExteriorRing");
impl_udf!(InteriorRingN, PyInteriorRingN, "InteriorRingN");
impl_udf!(PointN, PyPointN, "PointN");
impl_udf!(Points, PyPoints, "Points");
impl_udf!(Envelope, PyEnvelope, "Envelope");
impl_udf!(BoundingDiagonal, PyBoundingDiagonal, "BoundingDiagonal");
impl_udf!(IsPolygonCW, PyIsPolygonCW, "IsPolygonCW");
impl_udf!(IsPolygonCCW, PyIsPolygonCCW, "IsPolygonCCW");
