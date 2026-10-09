use geodatafusion::udf::native::measurement::{
    Area, Distance, Length, Length3D, Perimeter, Perimeter3D,
};

use crate::impl_udf;

impl_udf!(Area, PyArea, "Area");
impl_udf!(Distance, PyDistance, "Distance");
impl_udf!(Length, PyLength, "Length");
impl_udf!(Length3D, PyLength3D, "Length3D");
impl_udf!(Perimeter3D, PyPerimeter3D, "Perimeter3D");
impl_udf!(Perimeter, PyPerimeter, "Perimeter");
