use geodatafusion::udf::native::constructors::{
    Collect, CollectAgg, MakePoint, MakePointM, Point, PointM, PointZ, PointZM,
};

use crate::{impl_udaf, impl_udf};

impl_udf!(Collect, PyCollect, "Collect");
impl_udaf!(CollectAgg, PyCollectAgg, "CollectAgg");

impl_udf!(Point, PyPoint, "Point");
impl_udf!(PointZ, PyPointZ, "PointZ");
impl_udf!(PointM, PyPointM, "PointM");
impl_udf!(PointZM, PyPointZM, "PointZM");
impl_udf!(MakePoint, PyMakePoint, "MakePoint");
impl_udf!(MakePointM, PyMakePointM, "MakePointM");
