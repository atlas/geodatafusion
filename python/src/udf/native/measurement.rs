use geodatafusion::udf::native::measurement::Area;

use crate::impl_udf;

impl_udf!(Area, PyArea, "Area");
