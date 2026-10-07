use geodatafusion::udf::geo::validation::IsValid;

use crate::impl_udf;

impl_udf!(IsValid, PyIsValid, "IsValid");
