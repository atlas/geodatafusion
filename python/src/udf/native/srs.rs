use geodatafusion::udf::native::srs::{BestSRID, SRID, SetSRID};

use crate::impl_udf;

impl_udf!(BestSRID, PyBestSRID, "BestSRID");
impl_udf!(SetSRID, PySetSRID, "SetSRID");
impl_udf!(SRID, PySRID, "SRID");
