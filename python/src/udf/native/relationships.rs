use geodatafusion::udf::native::relationships::{
    DFullyWithin3D, DWithin, DWithin3D, Intersects3D, RelateMatch,
};

use crate::impl_udf;

impl_udf!(DWithin, PyDWithin, "DWithin");
impl_udf!(DFullyWithin3D, PyDFullyWithin3D, "DFullyWithin3D");
impl_udf!(DWithin3D, PyDWithin3D, "DWithin3D");
impl_udf!(Intersects3D, PyIntersects3D, "Intersects3D");
impl_udf!(RelateMatch, PyRelateMatch, "RelateMatch");
