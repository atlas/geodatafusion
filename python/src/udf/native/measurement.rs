use geodatafusion::udf::native::measurement::{
    Area, Azimuth, ClosestPoint, Distance, Length, Length3D, LongestLine, MaxDistance, Perimeter,
    Perimeter3D, ShortestLine,
};

use crate::impl_udf;

impl_udf!(Area, PyArea, "Area");
impl_udf!(Azimuth, PyAzimuth, "Azimuth");
impl_udf!(ClosestPoint, PyClosestPoint, "ClosestPoint");
impl_udf!(Distance, PyDistance, "Distance");
impl_udf!(Length, PyLength, "Length");
impl_udf!(Length3D, PyLength3D, "Length3D");
impl_udf!(LongestLine, PyLongestLine, "LongestLine");
impl_udf!(MaxDistance, PyMaxDistance, "MaxDistance");
impl_udf!(Perimeter3D, PyPerimeter3D, "Perimeter3D");
impl_udf!(Perimeter, PyPerimeter, "Perimeter");
impl_udf!(ShortestLine, PyShortestLine, "ShortestLine");
