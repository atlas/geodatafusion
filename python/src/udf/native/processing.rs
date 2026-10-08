use geodatafusion::udf::native::processing::{Simplify, SimplifyVW};

use crate::impl_udf;

impl_udf!(Simplify, PySimplify, "Simplify");
impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
