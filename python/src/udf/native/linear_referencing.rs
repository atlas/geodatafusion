use geodatafusion::udf::native::linear_referencing::{
    AddMeasure, InterpolatePoint, LineInterpolatePoint, LineInterpolatePoint3D,
    LineInterpolatePoints, LineLocatePoint, LineSubstring,
};

use crate::impl_udf;

impl_udf!(
    LineInterpolatePoint,
    PyLineInterpolatePoint,
    "LineInterpolatePoint"
);
impl_udf!(
    LineInterpolatePoint3D,
    PyLineInterpolatePoint3D,
    "LineInterpolatePoint3D"
);
impl_udf!(
    LineInterpolatePoints,
    PyLineInterpolatePoints,
    "LineInterpolatePoints"
);
impl_udf!(LineSubstring, PyLineSubstring, "LineSubstring");
impl_udf!(AddMeasure, PyAddMeasure, "AddMeasure");
impl_udf!(InterpolatePoint, PyInterpolatePoint, "InterpolatePoint");
impl_udf!(LineLocatePoint, PyLineLocatePoint, "LineLocatePoint");
