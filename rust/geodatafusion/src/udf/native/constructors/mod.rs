mod collect;
mod hexagon;
mod line_from_multi_point;
mod make_envelope;
mod make_line;
pub(crate) mod make_polygon;
mod point;
mod polygon;
mod tile_envelope;

pub use collect::{Collect, CollectAgg};
pub use hexagon::{Hexagon, Square};
pub use line_from_multi_point::LineFromMultiPoint;
pub use make_envelope::MakeEnvelope;
pub use make_line::{MakeLine, MakeLineAgg};
pub use make_polygon::MakePolygon;
pub use point::{MakePoint, MakePointM, Point, PointM, PointZ, PointZM};
pub use polygon::Polygon;
pub use tile_envelope::TileEnvelope;

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
    session_context.register_udf(TileEnvelope.into());
    session_context.register_udf(Hexagon.into());
    session_context.register_udf(Square.into());
    session_context.register_udf(LineFromMultiPoint.into());
}
