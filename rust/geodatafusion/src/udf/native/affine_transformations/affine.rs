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

/// PostGIS:
/// - ST_Affine(geometry geomA, float a, float b, float c, float d, float e, float f, float g,
///   float h, float i, float xoff, float yoff, float zoff)
/// - ST_Affine(geometry geomA, float a, float b, float d, float e, float xoff, float yoff)
///
/// PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[
    &[
        Arg::Geometry,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
    ],
    &[
        Arg::Geometry,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
    ],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Apply a 3D affine transformation to a geometry.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Applies a 3D affine transformation to a geometry: x' = a*x + b*y + c*z + xoff, y' = d*x + e*y + f*z + yoff, z' = g*x + h*y + i*z + zoff. The 2D form ST_Affine(geomA, a, b, d, e, xoff, yoff) leaves Z unchanged. A geometry without Z is transformed with Z 0 and stays 2D; M is unchanged.",
    syntax_example = "ST_Affine(geomA, a, b, c, d, e, f, g, h, i, xoff, yoff, zoff)",
    alternative_syntax = "ST_Affine(geomA, a, b, d, e, xoff, yoff)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "a", description = "float8"),
    argument(name = "b", description = "float8"),
    argument(name = "c", description = "float8"),
    argument(name = "d", description = "float8"),
    argument(name = "e", description = "float8"),
    argument(name = "f", description = "float8"),
    argument(name = "g", description = "float8"),
    argument(name = "h", description = "float8"),
    argument(name = "i", description = "float8"),
    argument(name = "xoff", description = "float8"),
    argument(name = "yoff", description = "float8"),
    argument(name = "zoff", description = "float8"),
    related_udf(name = "st_translate"),
    related_udf(name = "st_scale"),
    related_udf(name = "st_rotate")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Affine;

impl Affine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Affine {
    fn name(&self) -> &str {
        "st_affine"
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
        Ok(affine_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn affine_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let parameters = (1..args.args.len())
        .map(|index| optional_float_arg(&args, index, 0.0))
        .collect::<Result<Vec<_>>>()?;
    let kernel = AffineKernel { parameters };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// The 12 or 6 parameters of a call, in their SQL order.
struct AffineKernel {
    parameters: Vec<Float64Array>,
}

impl AffineKernel {
    fn affine(&self, row: usize) -> Option<Affine3D> {
        if self.parameters.iter().any(|array| array.is_null(row)) {
            return None;
        }
        let p: Vec<f64> = self
            .parameters
            .iter()
            .map(|array| array.value(row))
            .collect();
        Some(match p[..] {
            [a, b, c, d, e, f, g, h, i, xoff, yoff, zoff] => Affine3D {
                a,
                b,
                c,
                d,
                e,
                f,
                g,
                h,
                i,
                xoff,
                yoff,
                zoff,
            },
            // PostGIS's 2D form is the 3D one with Z left as it is.
            [a, b, d, e, xoff, yoff] => Affine3D {
                a,
                b,
                c: 0.0,
                d,
                e,
                f: 0.0,
                g: 0.0,
                h: 0.0,
                i: 1.0,
                xoff,
                yoff,
                zoff: 0.0,
            },
            _ => return None,
        })
    }
}

impl GeometryKernel for AffineKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        let Some(affine) = self.affine(row) else {
            return Ok(None);
        };
        Ok(Some(map_coords(geom, &|c| affine.apply(c))))
    }
}
