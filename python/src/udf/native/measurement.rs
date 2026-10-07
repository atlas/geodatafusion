use geodatafusion::udf::native::measurement::{Area, Distance};

use crate::impl_udf;

impl_udf!(Area, PyArea, "Area");
impl_udf!(Distance, PyDistance, "Distance");
