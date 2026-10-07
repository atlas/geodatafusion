use geodatafusion::udf::geo::processing::{Simplify, SimplifyPreserveTopology, SimplifyVW};

use crate::impl_udf;

impl_udf!(Simplify, PySimplify, "Simplify");
impl_udf!(
    SimplifyPreserveTopology,
    PySimplifyPreserveTopology,
    "SimplifyPreserveTopology"
);
impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
