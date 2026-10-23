use geodatafusion::udf::native::types::{Geography, Geometry};

use crate::impl_udf;

impl_udf!(Geometry, PyGeometry, "Geometry");
impl_udf!(Geography, PyGeography, "Geography");
