use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{Coord, Dimension};

use crate::error::GeoDataFusionResult;
use crate::util::args::{optional_bool_arg, optional_int_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{LinePart, dimension, map_line_strings};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ChaikinSmoothing(geometry geom, integer nIterations = 1,
/// boolean preserveEndPoints = false).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "nIterations", "preserveEndPoints"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a smoothed version of a geometry, using the Chaikin algorithm.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Smooths lines and polygon rings with Chaikin's corner cutting, nIterations times (1 to 5, default 1): each segment is replaced by the points a quarter and three quarters along it. Lines keep their end points; rings keep their first point only with preserveEndPoints (default false). Z is smoothed too, and M for XYZM geometries; as in PostGIS, the new points of an XYM geometry get M 0. Points are unchanged.",
    syntax_example = "ST_ChaikinSmoothing(geom, nIterations, preserveEndPoints)",
    argument(name = "geom", description = "geometry"),
    argument(name = "nIterations", description = "integer, default 1"),
    argument(name = "preserveEndPoints", description = "boolean, default false"),
    related_udf(name = "st_simplify"),
    related_udf(name = "st_segmentize")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ChaikinSmoothing;

impl ChaikinSmoothing {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ChaikinSmoothing {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ChaikinSmoothing {
    fn name(&self) -> &str {
        "st_chaikinsmoothing"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(chaikin_smoothing_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn chaikin_smoothing_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ChaikinSmoothingKernel {
        iterations: optional_int_arg(&args, 1, 1)?,
        preserve_end_points: optional_bool_arg(&args, 2, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ChaikinSmoothingKernel {
    iterations: Int32Array,
    preserve_end_points: BooleanArray,
}

impl GeometryKernel for ChaikinSmoothingKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.iterations.is_null(row) || self.preserve_end_points.is_null(row) {
            return Ok(None);
        }
        let iterations = self.iterations.value(row);
        if !(1..=5).contains(&iterations) {
            return Err(exec_datafusion_err!(
                "st_chaikinsmoothing: Number of iterations must be between 1 and 5"
            )
            .into());
        }
        let preserve = self.preserve_end_points.value(row);
        let dim = dimension(geom.dim());
        Ok(Some(map_line_strings(geom, &|part, mut coords| {
            let closed = part != LinePart::Line && !preserve;
            for _ in 0..iterations {
                coords = smooth(&coords, closed, dim);
            }
            coords
        })))
    }
}

/// One round of corner cutting. An open line keeps its end points; a closed ring is cut all
/// round and closed again.
fn smooth(coords: &[Coord<f64>], closed: bool, dim: Dimension) -> Vec<Coord<f64>> {
    if coords.len() < 3 {
        return coords.to_vec();
    }
    // PostGIS gives the new points of XYM geometries M 0.
    let smooth_m = dim == Dimension::XYZM;
    let mix = |a: &Coord<f64>, b: &Coord<f64>, wa: f64, wb: f64| Coord {
        x: wa * a.x + wb * b.x,
        y: wa * a.y + wb * b.y,
        z: a.z.zip(b.z).map(|(za, zb)| wa * za + wb * zb),
        m: a.m.map(|ma| {
            if smooth_m {
                wa * ma + wb * b.m.unwrap_or(0.0)
            } else {
                0.0
            }
        }),
    };
    let segments = coords.len() - 1;
    let mut out = Vec::with_capacity(segments * 2 + 1);
    if !closed {
        out.push(coords[0]);
    }
    for (index, pair) in coords.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        if closed || index > 0 {
            out.push(mix(a, b, 0.75, 0.25));
        }
        if closed || index + 1 < segments {
            out.push(mix(a, b, 0.25, 0.75));
        }
    }
    match (closed, out.first().copied()) {
        (true, Some(first)) => out.push(first),
        _ => out.push(coords[segments]),
    }
    out
}
