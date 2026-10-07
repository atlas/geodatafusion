use geodatafusion::udf::native::relationships::{DWithin, RelateMatch};

use crate::impl_udf;

impl_udf!(DWithin, PyDWithin, "DWithin");
impl_udf!(RelateMatch, PyRelateMatch, "RelateMatch");
