mod flip_coordinates;
mod swap_ordinates;

pub use flip_coordinates::FlipCoordinates;
pub use swap_ordinates::SwapOrdinates;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(FlipCoordinates.into());
    session_context.register_udf(SwapOrdinates.into());
}
