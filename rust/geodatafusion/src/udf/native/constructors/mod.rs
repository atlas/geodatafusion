mod collect;
mod make_envelope;
mod make_line;
pub(crate) mod make_polygon;
mod point;
mod polygon;

pub use collect::{Collect, CollectAgg};
pub use make_envelope::MakeEnvelope;
pub use make_line::{MakeLine, MakeLineAgg};
pub use make_polygon::MakePolygon;
pub use point::{MakePoint, MakePointM, Point, PointM, PointZ, PointZM};
pub use polygon::Polygon;

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Collect.into());
    session_context.register_udaf(CollectAgg.into());
    session_context.register_udf(MakeLine.into());
    session_context.register_udaf(MakeLineAgg.into());
    session_context.register_udf(MakePoint.into());
    session_context.register_udf(MakePointM.into());
    session_context.register_udf(Point.into());
    session_context.register_udf(PointM.into());
    session_context.register_udf(PointZ.into());
    session_context.register_udf(PointZM.into());
    session_context.register_udf(MakeEnvelope.into());
    session_context.register_udf(MakePolygon.into());
    session_context.register_udf(Polygon.into());
}
