//! ST_ClosestPointOfApproach, ST_DistanceCPA and ST_CPAWithin: when, and how near, two
//! trajectories pass each other.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, Dimensions, GeometryTrait, GeometryType, LineStringTrait};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::{m, z};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ClosestPointOfApproach(geometry track1, geometry track2), and the same for
/// ST_DistanceCPA.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

/// PostGIS: ST_CPAWithin(geometry track1, geometry track2, float8 dist).
static WITHIN_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["track1", "track2"])
        .expect("parameter names are valid for a user-defined signature")
});

static WITHIN_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["track1", "track2", "dist"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a measure at the closest point of approach of two trajectories.
#[user_doc(
    doc_section(label = "Trajectory Functions"),
    description = "Returns the M (time) at which two trajectories, linestrings with M, are nearest each other while both are moving: within the M range they share. Each moves linearly between its points. Distances are 3D if either has Z, a missing Z counting as 0. Returns NULL if the M ranges don't overlap; of equally near times, the earliest. The tracks should be valid trajectories (see ST_IsValidTrajectory): for others, such as tracks with repeated M values, the result can differ from PostGIS's.",
    syntax_example = "ST_ClosestPointOfApproach(track1, track2)",
    argument(name = "track1", description = "geometry"),
    argument(name = "track2", description = "geometry"),
    related_udf(name = "st_distancecpa"),
    related_udf(name = "st_cpawithin"),
    related_udf(name = "st_isvalidtrajectory")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ClosestPointOfApproach;

impl ClosestPointOfApproach {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClosestPointOfApproach {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ClosestPointOfApproach {
    fn name(&self) -> &str {
        "st_closestpointofapproach"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cpa_impl(args, Output::Time)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the distance between the closest point of approach of two trajectories.
#[user_doc(
    doc_section(label = "Trajectory Functions"),
    description = "Returns the smallest distance between two trajectories, linestrings with M, while both are moving: at their closest point of approach (see ST_ClosestPointOfApproach, also for invalid trajectories). Distances are 3D if either has Z, a missing Z counting as 0. Returns NULL if the M ranges don't overlap.",
    syntax_example = "ST_DistanceCPA(track1, track2)",
    alternative_syntax = "track1 |=| track2",
    argument(name = "track1", description = "geometry"),
    argument(name = "track2", description = "geometry"),
    related_udf(name = "st_closestpointofapproach"),
    related_udf(name = "st_cpawithin")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DistanceCPA;

impl DistanceCPA {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DistanceCPA {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DistanceCPA {
    fn name(&self) -> &str {
        "st_distancecpa"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cpa_impl(args, Output::Distance)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Tests if the closest point of approach of two trajectories is within the specified distance.
#[user_doc(
    doc_section(label = "Trajectory Functions"),
    description = "Returns true if two trajectories, linestrings with M, come within dist of each other while both are moving (see ST_DistanceCPA). Returns false if their M ranges don't overlap.",
    syntax_example = "ST_CPAWithin(track1, track2, dist)",
    argument(name = "track1", description = "geometry"),
    argument(name = "track2", description = "geometry"),
    argument(name = "dist", description = "float8"),
    related_udf(name = "st_distancecpa"),
    related_udf(name = "st_closestpointofapproach")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CPAWithin;

impl CPAWithin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CPAWithin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for CPAWithin {
    fn name(&self) -> &str {
        "st_cpawithin"
    }

    fn signature(&self) -> &Signature {
        &WITHIN_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, WITHIN_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cpa_within_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// What to return of the closest point of approach.
#[derive(Debug, Clone, Copy)]
enum Output {
    Time,
    Distance,
}

fn cpa_impl(args: ScalarFunctionArgs, output: Output) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = CpaKernel {
        track2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        output,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

fn cpa_within_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = CpaWithinKernel {
        track2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        dist: optional_float_arg(&args, 2, 0.0)?,
    };
    let result: BooleanArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct CpaKernel {
    track2: OwnedColumn,
    output: Output,
}

impl GeometryKernel for CpaKernel {
    type Output = f64;

    fn eval(
        &self,
        track1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(track2) = self.track2.get(row) else {
            return Ok(None);
        };
        Ok(
            closest_point_of_approach(track1, track2)?.map(|cpa| match self.output {
                Output::Time => cpa.time,
                Output::Distance => cpa.distance,
            }),
        )
    }
}

struct CpaWithinKernel {
    track2: OwnedColumn,
    dist: Float64Array,
}

impl GeometryKernel for CpaWithinKernel {
    type Output = bool;

    fn eval(
        &self,
        track1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        let Some(track2) = self.track2.get(row) else {
            return Ok(None);
        };
        if self.dist.is_null(row) {
            return Ok(None);
        }
        let cpa = closest_point_of_approach(track1, track2)?;
        Ok(Some(
            cpa.is_some_and(|cpa| cpa.distance <= self.dist.value(row)),
        ))
    }
}

/// The closest point of approach: when, and how far apart.
#[derive(Debug, Clone, Copy)]
struct Cpa {
    time: f64,
    distance: f64,
}

/// A point of a trajectory: x, y, z (0 without Z) and m.
type TrackPoint = [f64; 4];

/// The closest point of approach of two trajectories, or `None` if their M ranges don't overlap.
fn closest_point_of_approach(
    track1: &impl GeometryTrait<T = f64>,
    track2: &impl GeometryTrait<T = f64>,
) -> GeoDataFusionResult<Option<Cpa>> {
    let (points1, points2) = (track_points(track1)?, track_points(track2)?);
    // PostGIS measures in 3D if either track has Z.
    let use_z = has_z(track1) || has_z(track2);
    let (min1, max1) = m_range(&points1);
    let (min2, max2) = m_range(&points2);
    let (start, end) = (min1.max(min2), max1.min(max2));
    if start > end {
        return Ok(None);
    }
    // Both move linearly between the times either has a point at.
    let mut times: Vec<f64> = points1
        .iter()
        .chain(&points2)
        .map(|point| point[3])
        .filter(|&t| t > start && t < end)
        .collect();
    times.push(start);
    times.push(end);
    times.sort_by(f64::total_cmp);
    times.dedup();
    let distance = |a: [f64; 3]| {
        let squared = a[0] * a[0] + a[1] * a[1] + if use_z { a[2] * a[2] } else { 0.0 };
        squared.sqrt()
    };
    let separation = |t: f64| {
        let (p, q) = (position(&points1, t), position(&points2, t));
        [q[0] - p[0], q[1] - p[1], q[2] - p[2]]
    };
    if times.len() == 1 {
        return Ok(Some(Cpa {
            time: start,
            distance: distance(separation(start)),
        }));
    }
    let mut best: Option<Cpa> = None;
    for window in times.windows(2) {
        let (t0, t1) = (window[0], window[1]);
        let (d0, d1) = (separation(t0), separation(t1));
        let dd = [
            d1[0] - d0[0],
            d1[1] - d0[1],
            if use_z { d1[2] - d0[2] } else { 0.0 },
        ];
        let dd_squared = dd[0] * dd[0] + dd[1] * dd[1] + dd[2] * dd[2];
        let s = if dd_squared == 0.0 {
            0.0
        } else {
            let dot = d0[0] * dd[0] + d0[1] * dd[1] + if use_z { d0[2] * dd[2] } else { 0.0 };
            (-dot / dd_squared).clamp(0.0, 1.0)
        };
        let cpa = Cpa {
            time: t0 + s * (t1 - t0),
            distance: distance([d0[0] + s * dd[0], d0[1] + s * dd[1], d0[2] + s * dd[2]]),
        };
        if best.is_none_or(|best| cpa.distance < best.distance) {
            best = Some(cpa);
        }
    }
    Ok(best)
}

fn has_z(geom: &impl GeometryTrait<T = f64>) -> bool {
    matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm)
}

/// The points of a trajectory, with PostGIS's errors for anything else.
fn track_points(geom: &impl GeometryTrait<T = f64>) -> GeoDataFusionResult<Vec<TrackPoint>> {
    let GeometryType::LineString(line) = geom.as_type() else {
        return Err(exec_datafusion_err!("Both input geometries must be linestrings").into());
    };
    if !matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
        return Err(
            exec_datafusion_err!("Both input geometries must have a measure dimension").into(),
        );
    }
    let points: Vec<TrackPoint> = line
        .coords()
        .map(|c| [c.x(), c.y(), z(&c).unwrap_or(0.0), m(&c).unwrap_or(0.0)])
        .collect();
    if points.len() < 2 {
        return Err(exec_datafusion_err!("Both input lines must have at least 2 points").into());
    }
    Ok(points)
}

fn m_range(points: &[TrackPoint]) -> (f64, f64) {
    points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[3]), hi.max(p[3]))
        })
}

/// Where the trajectory is at time `t`: on the first segment whose M range holds `t`.
fn position(points: &[TrackPoint], t: f64) -> [f64; 3] {
    for segment in points.windows(2) {
        let (a, b) = (segment[0], segment[1]);
        let (lo, hi) = (a[3].min(b[3]), a[3].max(b[3]));
        if t < lo || t > hi {
            continue;
        }
        if a[3] == b[3] {
            return [a[0], a[1], a[2]];
        }
        let f = (t - a[3]) / (b[3] - a[3]);
        return [
            a[0] + (b[0] - a[0]) * f,
            a[1] + (b[1] - a[1]) * f,
            a[2] + (b[2] - a[2]) * f,
        ];
    }
    let last = points[points.len() - 1];
    [last[0], last[1], last[2]]
}
