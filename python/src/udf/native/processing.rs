use geodatafusion::udf::native::processing::{ChaikinSmoothing, FilterByM, Simplify, SimplifyVW};

use crate::impl_udf;

impl_udf!(Simplify, PySimplify, "Simplify");
impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
impl_udf!(ChaikinSmoothing, PyChaikinSmoothing, "ChaikinSmoothing");
impl_udf!(FilterByM, PyFilterByM, "FilterByM");
