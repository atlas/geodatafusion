use geodatafusion::udf::native::processing::Simplify;

use crate::impl_udf;

impl_udf!(Simplify, PySimplify, "Simplify");
