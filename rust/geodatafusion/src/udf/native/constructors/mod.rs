mod point;

pub use point::{MakePoint, MakePointM, Point, PointM, PointZ, PointZM};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(MakePoint.into());
    session_context.register_udf(MakePointM.into());
    session_context.register_udf(Point.into());
    session_context.register_udf(PointM.into());
    session_context.register_udf(PointZ.into());
    session_context.register_udf(PointZM.into());
}
