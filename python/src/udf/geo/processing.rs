use geodatafusion::udf::geo::processing::{Simplify, SimplifyVW};

use crate::impl_udf;

impl_udf!(Simplify, PySimplify, "Simplify");
impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
