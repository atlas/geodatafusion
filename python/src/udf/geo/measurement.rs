use geodatafusion::udf::geo::measurement::{Distance, Length};

use crate::impl_udf;

impl_udf!(Distance, PyDistance, "Distance");
impl_udf!(Length, PyLength, "Length");
