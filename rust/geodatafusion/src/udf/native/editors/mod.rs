mod flip_coordinates;
mod force;
mod force_polygon_cw;
mod quantize_coordinates;
mod reverse;
mod shift_longitude;
mod snap_to_grid;
mod swap_ordinates;

pub use flip_coordinates::FlipCoordinates;
pub use force::{Force2D, Force3DM, Force3DZ, Force4D};
pub use force_polygon_cw::{ForcePolygonCCW, ForcePolygonCW};
pub use quantize_coordinates::QuantizeCoordinates;
pub use reverse::Reverse;
pub use shift_longitude::ShiftLongitude;
pub use snap_to_grid::SnapToGrid;
pub use swap_ordinates::SwapOrdinates;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(FlipCoordinates.into());
    session_context.register_udf(SwapOrdinates.into());
    session_context.register_udf(Force2D.into());
    session_context.register_udf(Force3DZ.into());
    session_context.register_udf(Force3DM.into());
    session_context.register_udf(Force4D.into());
    session_context.register_udf(ShiftLongitude.into());
    session_context.register_udf(Reverse.into());
    session_context.register_udf(ForcePolygonCW.into());
    session_context.register_udf(ForcePolygonCCW.into());
    session_context.register_udf(SnapToGrid.into());
    session_context.register_udf(QuantizeCoordinates.into());
}
