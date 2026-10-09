//! ST_Rotate, ST_RotateX, ST_RotateY and ST_RotateZ: rotations, which PostGIS defines as
//! ST_Affine calls.

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
use geo_traits::GeometryTrait;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::native::affine_transformations::util::affine::Affine3D;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, map_coords};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_Rotate(geometry geomA, float rotRadians)
/// - ST_Rotate(geometry geomA, float rotRadians, float x0, float y0)
/// - ST_Rotate(geometry geomA, float rotRadians, geometry pointOrigin)
///
/// PostGIS doesn't name the parameters.
static ROTATE_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Geometry],
];

/// PostGIS: ST_RotateX(geometry geomA, float rotRadians), and the same for ST_RotateY and
/// ST_RotateZ.
static ROTATE_AXIS_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Rotates a geometry about an origin point.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Rotates a geometry rotRadians counter-clockwise about the origin, about (x0, y0), or about a POINT. Z and M are unchanged. An empty origin POINT gives NULL, as in PostGIS, and its SRID isn't checked.",
    syntax_example = "ST_Rotate(geomA, rotRadians, x0, y0)",
    alternative_syntax = "ST_Rotate(geomA, rotRadians, pointOrigin)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "rotRadians", description = "float8"),
    argument(name = "x0", description = "float8, or pointOrigin: a POINT geometry"),
    argument(name = "y0", description = "float8"),
    related_udf(name = "st_affine"),
    related_udf(name = "st_rotatex"),
    related_udf(name = "st_rotatey"),
    related_udf(name = "st_rotatez")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Rotate;

impl Rotate {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Rotate {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Rotate {
    fn name(&self) -> &str {
        "st_rotate"
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
        coerce_args(self.name(), arg_types, ROTATE_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(rotate_impl(args, Axis::Z)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Rotates a geometry about the X axis.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Rotates a geometry rotRadians about the X axis. A geometry without Z is rotated with Z 0 and stays 2D; M is unchanged.",
    syntax_example = "ST_RotateX(geomA, rotRadians)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "rotRadians", description = "float8"),
    related_udf(name = "st_rotate"),
    related_udf(name = "st_rotatey"),
    related_udf(name = "st_rotatez")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RotateX;

impl RotateX {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RotateX {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RotateX {
    fn name(&self) -> &str {
        "st_rotatex"
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
        coerce_args(self.name(), arg_types, ROTATE_AXIS_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(rotate_impl(args, Axis::X)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Rotates a geometry about the Y axis.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Rotates a geometry rotRadians about the Y axis. A geometry without Z is rotated with Z 0 and stays 2D; M is unchanged.",
    syntax_example = "ST_RotateY(geomA, rotRadians)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "rotRadians", description = "float8"),
    related_udf(name = "st_rotate"),
    related_udf(name = "st_rotatex"),
    related_udf(name = "st_rotatez")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RotateY;

impl RotateY {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RotateY {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RotateY {
    fn name(&self) -> &str {
        "st_rotatey"
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
        coerce_args(self.name(), arg_types, ROTATE_AXIS_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(rotate_impl(args, Axis::Y)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Rotates a geometry about the Z axis.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Rotates a geometry rotRadians about the Z axis: the same as ST_Rotate(geomA, rotRadians). Z and M are unchanged.",
    syntax_example = "ST_RotateZ(geomA, rotRadians)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "rotRadians", description = "float8"),
    related_udf(name = "st_rotate"),
    related_udf(name = "st_rotatex"),
    related_udf(name = "st_rotatey")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RotateZ;

impl RotateZ {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RotateZ {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RotateZ {
    fn name(&self) -> &str {
        "st_rotatez"
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
        coerce_args(self.name(), arg_types, ROTATE_AXIS_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(rotate_impl(args, Axis::Z)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

/// Where a rotation about the Z axis is centred.
enum Origin {
    Zero,
    Coordinates(Float64Array, Float64Array),
    Point(OwnedColumn),
}

fn rotate_impl(args: ScalarFunctionArgs, axis: Axis) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let origin = match args.args.len() {
        4 => Origin::Coordinates(
            optional_float_arg(&args, 2, 0.0)?,
            optional_float_arg(&args, 3, 0.0)?,
        ),
        3 => Origin::Point(OwnedColumn::try_new(
            &args.args[2],
            &args.arg_fields[2],
            args.number_rows,
        )?),
        _ => Origin::Zero,
    };
    let kernel = RotateKernel {
        axis,
        angle: optional_float_arg(&args, 1, 0.0)?,
        origin,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct RotateKernel {
    axis: Axis,
    angle: Float64Array,
    origin: Origin,
}

impl RotateKernel {
    /// The origin in row `row`; `None` if it is NULL, or an empty POINT (PostGIS reads it with
    /// ST_X and ST_Y, which return NULL for it).
    fn origin(&self, row: usize) -> GeoDataFusionResult<Option<(f64, f64)>> {
        Ok(match &self.origin {
            Origin::Zero => Some((0.0, 0.0)),
            Origin::Coordinates(x0, y0) => {
                (!x0.is_null(row) && !y0.is_null(row)).then(|| (x0.value(row), y0.value(row)))
            }
            Origin::Point(points) => match points.get(row) {
                None => None,
                Some(Wkt::Point(point)) => point.coord().map(|coord| (coord.x, coord.y)),
                Some(_) => {
                    return Err(exec_datafusion_err!(
                        "st_rotate: Argument to ST_X() must have type POINT"
                    )
                    .into());
                }
            },
        })
    }
}

impl GeometryKernel for RotateKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.angle.is_null(row) {
            return Ok(None);
        }
        let Some((x0, y0)) = self.origin(row)? else {
            return Ok(None);
        };
        let angle = self.angle.value(row);
        let (sin, cos) = (angle.sin(), angle.cos());
        // The matrices of PostGIS's SQL definitions, with the offsets computed as it does.
        let identity = Affine3D::translate(0.0, 0.0, 0.0);
        let affine = match self.axis {
            Axis::X => Affine3D {
                e: cos,
                f: -sin,
                h: sin,
                i: cos,
                ..identity
            },
            Axis::Y => Affine3D {
                a: cos,
                c: sin,
                g: -sin,
                i: cos,
                ..identity
            },
            Axis::Z => Affine3D {
                a: cos,
                b: -sin,
                d: sin,
                e: cos,
                xoff: x0 - cos * x0 + sin * y0,
                yoff: y0 - sin * x0 - cos * y0,
                ..identity
            },
        };
        Ok(Some(map_coords(geom, &|c| affine.apply(c))))
    }
}
