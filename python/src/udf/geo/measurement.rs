use geodatafusion::udf::geo::measurement::Length;

use crate::impl_udf;

impl_udf!(Length, PyLength, "Length");
