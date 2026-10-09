use geodatafusion::udf::native::editors::{
    FlipCoordinates, Force2D, Force3DM, Force3DZ, Force4D, ForcePolygonCCW, ForcePolygonCW,
    QuantizeCoordinates, Reverse, ShiftLongitude, SnapToGrid, SwapOrdinates,
};

use crate::impl_udf;

impl_udf!(FlipCoordinates, PyFlipCoordinates, "FlipCoordinates");
impl_udf!(SwapOrdinates, PySwapOrdinates, "SwapOrdinates");
impl_udf!(Force2D, PyForce2D, "Force2D");
impl_udf!(Force3DZ, PyForce3DZ, "Force3DZ");
impl_udf!(Force3DM, PyForce3DM, "Force3DM");
impl_udf!(Force4D, PyForce4D, "Force4D");
impl_udf!(ShiftLongitude, PyShiftLongitude, "ShiftLongitude");
impl_udf!(Reverse, PyReverse, "Reverse");
impl_udf!(ForcePolygonCW, PyForcePolygonCW, "ForcePolygonCW");
impl_udf!(ForcePolygonCCW, PyForcePolygonCCW, "ForcePolygonCCW");
impl_udf!(SnapToGrid, PySnapToGrid, "SnapToGrid");
impl_udf!(
    QuantizeCoordinates,
    PyQuantizeCoordinates,
    "QuantizeCoordinates"
);
