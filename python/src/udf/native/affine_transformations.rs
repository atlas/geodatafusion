use geodatafusion::udf::native::affine_transformations::{Affine, Scale, Translate};

use crate::impl_udf;

impl_udf!(Affine, PyAffine, "Affine");
impl_udf!(Translate, PyTranslate, "Translate");
impl_udf!(Scale, PyScale, "Scale");
