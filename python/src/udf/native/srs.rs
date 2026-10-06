use geodatafusion::udf::native::srs::{SRID, SetSRID};

use crate::impl_udf;

impl_udf!(SetSRID, PySetSRID, "SetSRID");
impl_udf!(SRID, PySRID, "SRID");
