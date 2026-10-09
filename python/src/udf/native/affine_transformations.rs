use geodatafusion::udf::native::affine_transformations::{Affine, Translate};

use crate::impl_udf;

impl_udf!(Affine, PyAffine, "Affine");
impl_udf!(Translate, PyTranslate, "Translate");
