use geodatafusion::udf::native::constructors::{
    MakePoint, MakePointM, Point, PointM, PointZ, PointZM,
};

use crate::impl_udf;

impl_udf!(Point, PyPoint, "Point");
impl_udf!(PointZ, PyPointZ, "PointZ");
impl_udf!(PointM, PyPointM, "PointM");
impl_udf!(PointZM, PyPointZM, "PointZM");
impl_udf!(MakePoint, PyMakePoint, "MakePoint");
impl_udf!(MakePointM, PyMakePointM, "MakePointM");
