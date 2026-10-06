use geodatafusion::udf::geo::processing::{
    Centroid, ConvexHull, OrientedEnvelope, PointOnSurface, Simplify, SimplifyPreserveTopology,
    SimplifyVW,
};

use crate::impl_udf;

impl_udf!(Centroid, PyCentroid, "Centroid");
impl_udf!(ConvexHull, PyConvexHull, "ConvexHull");
impl_udf!(OrientedEnvelope, PyOrientedEnvelope, "OrientedEnvelope");
impl_udf!(PointOnSurface, PyPointOnSurface, "PointOnSurface");
impl_udf!(Simplify, PySimplify, "Simplify");
impl_udf!(
    SimplifyPreserveTopology,
    PySimplifyPreserveTopology,
    "SimplifyPreserveTopology"
);
impl_udf!(SimplifyVW, PySimplifyVW, "SimplifyVW");
