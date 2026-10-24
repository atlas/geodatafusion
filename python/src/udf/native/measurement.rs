use geodatafusion::udf::native::measurement::{
    Area, Azimuth, ClosestPoint, ClosestPoint3D, Distance, Distance3D, DistanceSphere,
    DistanceSpheroid, Length, Length3D, LengthSpheroid, LongestLine, LongestLine3D, MaxDistance,
    MaxDistance3D, Perimeter, Perimeter3D, ShortestLine, ShortestLine3D,
};

use crate::impl_udf;

impl_udf!(Area, PyArea, "Area");
impl_udf!(Azimuth, PyAzimuth, "Azimuth");
impl_udf!(ClosestPoint, PyClosestPoint, "ClosestPoint");
impl_udf!(ClosestPoint3D, PyClosestPoint3D, "ClosestPoint3D");
impl_udf!(Distance, PyDistance, "Distance");
impl_udf!(Distance3D, PyDistance3D, "Distance3D");
impl_udf!(DistanceSphere, PyDistanceSphere, "DistanceSphere");
impl_udf!(DistanceSpheroid, PyDistanceSpheroid, "DistanceSpheroid");
impl_udf!(Length, PyLength, "Length");
impl_udf!(Length3D, PyLength3D, "Length3D");
impl_udf!(LengthSpheroid, PyLengthSpheroid, "LengthSpheroid");
impl_udf!(LongestLine, PyLongestLine, "LongestLine");
impl_udf!(LongestLine3D, PyLongestLine3D, "LongestLine3D");
impl_udf!(MaxDistance, PyMaxDistance, "MaxDistance");
impl_udf!(MaxDistance3D, PyMaxDistance3D, "MaxDistance3D");
impl_udf!(Perimeter3D, PyPerimeter3D, "Perimeter3D");
impl_udf!(Perimeter, PyPerimeter, "Perimeter");
impl_udf!(ShortestLine, PyShortestLine, "ShortestLine");
impl_udf!(ShortestLine3D, PyShortestLine3D, "ShortestLine3D");
