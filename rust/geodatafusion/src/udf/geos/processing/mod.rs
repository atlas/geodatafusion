mod buffer;
mod build_area;
mod centroid;
mod concave_hull;
mod convex_hull;
mod delaunay_triangles;
mod line_merge;
mod offset_curve;
mod oriented_envelope;
mod point_on_surface;
mod reduce_precision;
mod simplify_preserve_topology;
mod voronoi;

pub use buffer::Buffer;
pub use build_area::BuildArea;
pub use centroid::Centroid;
pub use concave_hull::ConcaveHull;
pub use convex_hull::ConvexHull;
pub use delaunay_triangles::DelaunayTriangles;
pub use line_merge::LineMerge;
pub use offset_curve::OffsetCurve;
pub use oriented_envelope::OrientedEnvelope;
pub use point_on_surface::PointOnSurface;
pub use reduce_precision::ReducePrecision;
pub use simplify_preserve_topology::SimplifyPreserveTopology;
pub use voronoi::{VoronoiLines, VoronoiPolygons};

pub fn register(session_context: &datafusion::prelude::SessionContext) {
    session_context.register_udf(Buffer.into());
    session_context.register_udf(BuildArea.into());
    session_context.register_udf(Centroid.into());
    session_context.register_udf(ConcaveHull.into());
    session_context.register_udf(ConvexHull.into());
    session_context.register_udf(DelaunayTriangles.into());
    session_context.register_udf(LineMerge.into());
    session_context.register_udf(OffsetCurve.into());
    session_context.register_udf(OrientedEnvelope.into());
    session_context.register_udf(PointOnSurface.into());
    session_context.register_udf(ReducePrecision.into());
    session_context.register_udf(SimplifyPreserveTopology.into());
    session_context.register_udf(VoronoiLines.into());
    session_context.register_udf(VoronoiPolygons.into());
}
