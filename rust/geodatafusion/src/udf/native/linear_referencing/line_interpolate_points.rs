use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::Wkt;
use wkt::types::{MultiPoint, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::native::linear_referencing::util::walk::{Length, interpolate};
use crate::util::args::{optional_bool_arg, optional_float_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineInterpolatePoints(geometry a_linestring, float8 a_fraction,
/// boolean repeat = true). The geography form waits for the geography type.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_linestring", "a_fraction", "repeat"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns points interpolated along a line at a fractional interval.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the points at every a_fraction (0 to 1) of the 2D length of a LINESTRING, interpolating Z and M: a MULTIPOINT, or a POINT if there is one. With repeat false, or a fraction of 0, only the first point is returned. The fractions are added up as PostGIS adds them, so they carry its rounding. An empty line gives POINT EMPTY.",
    syntax_example = "ST_LineInterpolatePoints(a_linestring, a_fraction, repeat)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "a_fraction", description = "float8 from 0 to 1"),
    argument(name = "repeat", description = "boolean, default true"),
    related_udf(name = "st_lineinterpolatepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineInterpolatePoints;

impl LineInterpolatePoints {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineInterpolatePoints {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineInterpolatePoints {
    fn name(&self) -> &str {
        "st_lineinterpolatepoints"
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
        Ok(line_interpolate_points_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_interpolate_points_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineInterpolatePointsKernel {
        fraction: optional_float_arg(&args, 1, 0.0)?,
        repeat: optional_bool_arg(&args, 2, true)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineInterpolatePointsKernel {
    fraction: Float64Array,
    repeat: BooleanArray,
}

impl GeometryKernel for LineInterpolatePointsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.fraction.is_null(row) || self.repeat.is_null(row) {
            return Ok(None);
        }
        let fraction = self.fraction.value(row);
        if !(0.0..=1.0).contains(&fraction) {
            return Err(exec_datafusion_err!(
                "st_lineinterpolatepoints: line_interpolate_point: 2nd arg isn't within [0,1]"
            )
            .into());
        }
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "st_lineinterpolatepoints: line_interpolate_point: 1st arg isn't a line"
            )
            .into());
        };
        let dim = dimension(geom.dim());
        let (coords, _) = owned_line_string(line, dim).into_inner();
        if coords.is_empty() {
            return Ok(Some(Wkt::Point(Point::new(None, dim))));
        }
        let count = if self.repeat.value(row) && fraction > 0.0 {
            (1.0 / fraction).floor() as usize
        } else {
            1
        };
        // PostGIS adds the fraction up rather than multiplying it.
        let mut points = Vec::with_capacity(count);
        let mut at = 0.0;
        for _ in 0..count {
            at += fraction;
            points.push(Point::new(interpolate(&coords, at, Length::Planar), dim));
        }
        Ok(Some(match <[_; 1]>::try_from(points) {
            Ok([point]) => Wkt::Point(point),
            Err(points) => Wkt::MultiPoint(MultiPoint::new(points, dim)),
        }))
    }
}
