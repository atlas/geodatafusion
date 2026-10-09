use geodatafusion::udf::native::linear_referencing::{
    LineInterpolatePoint, LineInterpolatePoint3D, LineInterpolatePoints, LineSubstring,
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
