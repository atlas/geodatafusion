use geodatafusion::udf::native::constructors::{
    Collect, CollectAgg, MakeEnvelope, MakeLine, MakeLineAgg, MakePoint, MakePointM, Point, PointM,
    PointZ, PointZM,
};

use crate::{impl_udaf, impl_udf};

impl_udf!(Collect, PyCollect, "Collect");
impl_udaf!(CollectAgg, PyCollectAgg, "CollectAgg");
impl_udf!(MakeLine, PyMakeLine, "MakeLine");
impl_udaf!(MakeLineAgg, PyMakeLineAgg, "MakeLineAgg");

impl_udf!(Point, PyPoint, "Point");
impl_udf!(PointZ, PyPointZ, "PointZ");
impl_udf!(PointM, PyPointM, "PointM");
impl_udf!(PointZM, PyPointZM, "PointZM");
impl_udf!(MakePoint, PyMakePoint, "MakePoint");
impl_udf!(MakePointM, PyMakePointM, "MakePointM");
impl_udf!(MakeEnvelope, PyMakeEnvelope, "MakeEnvelope");
