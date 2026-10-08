use geodatafusion::udf::geo::processing::SimplifyVW;

use crate::impl_udf;

impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
