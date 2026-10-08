use geodatafusion::udf::native::types::Geometry;

use crate::impl_udf;

impl_udf!(Geometry, PyGeometry, "Geometry");
