mod coord_dim;
mod dump;
mod geometry_type;
mod is_closed;
mod is_empty;
mod line_string;
mod npoints;
mod num_interior_rings;
mod point;

pub use coord_dim::{CoordDim, NDims};
pub use dump::Dump;
pub use geometry_type::{GeometryType, ST_GeometryType};
pub use is_closed::IsClosed;
pub use is_empty::IsEmpty;
pub use line_string::{EndPoint, StartPoint};
pub use npoints::{NPoints, NumPoints};
pub use num_interior_rings::NumInteriorRings;
pub use point::{M, X, Y, Z};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(CoordDim.into());
    session_context.register_udf(NDims.into());
    session_context.register_udf(GeometryType.into());
    session_context.register_udf(ST_GeometryType.into());
    session_context.register_udf(IsClosed.into());
    session_context.register_udf(IsEmpty.into());
    session_context.register_udf(Dump.into());
    session_context.register_udf(EndPoint.into());
    session_context.register_udf(StartPoint.into());
    session_context.register_udf(NPoints.into());
    session_context.register_udf(NumPoints.into());
    session_context.register_udf(NumInteriorRings.into());
    session_context.register_udf(M.into());
    session_context.register_udf(X.into());
    session_context.register_udf(Y.into());
    session_context.register_udf(Z.into());
}
