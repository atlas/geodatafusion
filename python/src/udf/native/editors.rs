use geodatafusion::udf::native::editors::{FlipCoordinates, SwapOrdinates};

use crate::impl_udf;

impl_udf!(FlipCoordinates, PyFlipCoordinates, "FlipCoordinates");
impl_udf!(SwapOrdinates, PySwapOrdinates, "SwapOrdinates");
