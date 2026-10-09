use geodatafusion::udf::native::affine_transformations::{
    Affine, Rotate, RotateX, RotateY, RotateZ, Scale, TransScale, Translate,
};

use crate::impl_udf;

impl_udf!(Affine, PyAffine, "Affine");
impl_udf!(Translate, PyTranslate, "Translate");
impl_udf!(Scale, PyScale, "Scale");
impl_udf!(Rotate, PyRotate, "Rotate");
impl_udf!(RotateX, PyRotateX, "RotateX");
impl_udf!(RotateY, PyRotateY, "RotateY");
impl_udf!(RotateZ, PyRotateZ, "RotateZ");
impl_udf!(TransScale, PyTransScale, "TransScale");
