use geodatafusion::udf::native::operators::{
    GeometryAbove,
    GeometryBelow,
    GeometryContains,
    GeometryDistanceBox,
    GeometryLeft,
    GeometryOverAbove,
    GeometryOverBelow,
    GeometryOverLeft,
    GeometryOverRight,
    GeometryOverlaps,
    GeometryOverlapsNd,
    GeometryRight,
    GeometrySame,
    GeometryWithin,
};

use crate::impl_udf;

impl_udf!(GeometryAbove, PyGeometryAbove, "GeometryAbove");
impl_udf!(GeometryBelow, PyGeometryBelow, "GeometryBelow");
impl_udf!(GeometryContains, PyGeometryContains, "GeometryContains");
impl_udf!(GeometryDistanceBox, PyGeometryDistanceBox, "GeometryDistanceBox");
impl_udf!(GeometryLeft, PyGeometryLeft, "GeometryLeft");
impl_udf!(GeometryOverAbove, PyGeometryOverAbove, "GeometryOverAbove");
impl_udf!(GeometryOverBelow, PyGeometryOverBelow, "GeometryOverBelow");
impl_udf!(GeometryOverLeft, PyGeometryOverLeft, "GeometryOverLeft");
impl_udf!(GeometryOverRight, PyGeometryOverRight, "GeometryOverRight");
impl_udf!(GeometryOverlaps, PyGeometryOverlaps, "GeometryOverlaps");
impl_udf!(GeometryOverlapsNd, PyGeometryOverlapsNd, "GeometryOverlapsNd");
impl_udf!(GeometryRight, PyGeometryRight, "GeometryRight");
impl_udf!(GeometrySame, PyGeometrySame, "GeometrySame");
impl_udf!(GeometryWithin, PyGeometryWithin, "GeometryWithin");
