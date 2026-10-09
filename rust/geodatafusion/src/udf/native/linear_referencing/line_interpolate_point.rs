//! ST_LineInterpolatePoint and ST_3DLineInterpolatePoint: the point at a fraction of a line.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::types::Point;

use crate::error::GeoDataFusionResult;
use crate::udf::native::linear_referencing::util::walk::{Length, interpolate};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineInterpolatePoint(geometry a_linestring, float8 a_fraction), and the same for
/// ST_3DLineInterpolatePoint. The geography form waits for the geography type.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_linestring", "a_fraction"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a point interpolated along a line at a fractional location.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the POINT at a_fraction (0 to 1) of the 2D length of a LINESTRING, interpolating Z and M. An empty line gives POINT EMPTY. The geography form isn't supported yet.",
    syntax_example = "ST_LineInterpolatePoint(a_linestring, a_fraction)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "a_fraction", description = "float8 from 0 to 1"),
    related_udf(name = "st_lineinterpolatepoints"),
    related_udf(name = "st_linesubstring"),
    related_udf(name = "st_3dlineinterpolatepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineInterpolatePoint;

impl LineInterpolatePoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineInterpolatePoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineInterpolatePoint {
    fn name(&self) -> &str {
        "st_lineinterpolatepoint"
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
        Ok(line_interpolate_point_impl(args, Length::Planar)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a point interpolated along a 3D line at a fractional location.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the POINT at a_fraction (0 to 1) of the length of a LINESTRING, measured in 3D when it has Z, interpolating Z and M. An empty line gives POINT EMPTY.",
    syntax_example = "ST_3DLineInterpolatePoint(a_linestring, a_fraction)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "a_fraction", description = "float8 from 0 to 1"),
    related_udf(name = "st_lineinterpolatepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineInterpolatePoint3D;

impl LineInterpolatePoint3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineInterpolatePoint3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineInterpolatePoint3D {
    fn name(&self) -> &str {
        "st_3dlineinterpolatepoint"
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
        Ok(line_interpolate_point_impl(args, Length::Spatial)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_interpolate_point_impl(
    args: ScalarFunctionArgs,
    length: Length,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineInterpolatePointKernel {
        fraction: optional_float_arg(&args, 1, 0.0)?,
        length,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineInterpolatePointKernel {
    fraction: Float64Array,
    length: Length,
}

impl GeometryKernel for LineInterpolatePointKernel {
    type Output = Point<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Point<f64>>> {
        if self.fraction.is_null(row) {
            return Ok(None);
        }
        let fraction = self.fraction.value(row);
        let name = match self.length {
            Length::Planar => "st_lineinterpolatepoint",
            Length::Spatial => "st_3dlineinterpolatepoint",
        };
        if !(0.0..=1.0).contains(&fraction) {
            return Err(exec_datafusion_err!(
                "{name}: line_interpolate_point: 2nd arg isn't within [0,1]"
            )
            .into());
        }
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "{name}: line_interpolate_point: 1st arg isn't a line"
            )
            .into());
        };
        let dim = dimension(geom.dim());
        let (coords, _) = owned_line_string(line, dim).into_inner();
        Ok(Some(Point::new(
            interpolate(&coords, fraction, self.length),
            dim,
        )))
    }
}
