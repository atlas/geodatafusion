use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
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
use crate::util::owned::map_coords;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_TransScale(geometry geomA, float deltaX, float deltaY, float XFactor,
/// float YFactor). PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[
    Arg::Geometry,
    Arg::Float,
    Arg::Float,
    Arg::Float,
    Arg::Float,
]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Translates and scales a geometry by given offsets and factors.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Translates a geometry by deltaX and deltaY, then scales it by XFactor and YFactor: x' = (x + deltaX) * XFactor, y' = (y + deltaY) * YFactor. Works in 2D only; Z and M are unchanged.",
    syntax_example = "ST_TransScale(geomA, deltaX, deltaY, XFactor, YFactor)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "deltaX", description = "float8"),
    argument(name = "deltaY", description = "float8"),
    argument(name = "XFactor", description = "float8"),
    argument(name = "YFactor", description = "float8"),
    related_udf(name = "st_affine"),
    related_udf(name = "st_translate"),
    related_udf(name = "st_scale")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct TransScale;

impl TransScale {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TransScale {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for TransScale {
    fn name(&self) -> &str {
        "st_transscale"
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
        Ok(trans_scale_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn trans_scale_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = TransScaleKernel {
        delta_x: optional_float_arg(&args, 1, 0.0)?,
        delta_y: optional_float_arg(&args, 2, 0.0)?,
        x_factor: optional_float_arg(&args, 3, 1.0)?,
        y_factor: optional_float_arg(&args, 4, 1.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct TransScaleKernel {
    delta_x: Float64Array,
    delta_y: Float64Array,
    x_factor: Float64Array,
    y_factor: Float64Array,
}

impl GeometryKernel for TransScaleKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let arrays = [&self.delta_x, &self.delta_y, &self.x_factor, &self.y_factor];
        // SQL NULL in any argument, SQL NULL out.
        if arrays.iter().any(|array| array.is_null(row)) {
            return Ok(None);
        }
        let [delta_x, delta_y, x_factor, y_factor] = arrays.map(|array| array.value(row));
        // PostGIS's SQL definition of ST_TransScale.
        let affine = Affine3D {
            a: x_factor,
            e: y_factor,
            xoff: delta_x * x_factor,
            yoff: delta_y * y_factor,
            ..Affine3D::translate(0.0, 0.0, 0.0)
        };
        Ok(Some(map_coords(geom, &|c| affine.apply(c))))
    }
}
