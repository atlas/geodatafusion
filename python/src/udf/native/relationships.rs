use geodatafusion::udf::native::relationships::RelateMatch;

use crate::impl_udf;

impl_udf!(RelateMatch, PyRelateMatch, "RelateMatch");
