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
/// - ST_Translate(geometry g1, float deltax, float deltay)
/// - ST_Translate(geometry g1, float deltax, float deltay, float deltaz)
///
/// PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Translates a geometry by given offsets.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Returns a geometry whose coordinates are translated by deltax, deltay and deltaz. deltaz only moves geometries with Z; M is unchanged.",
    syntax_example = "ST_Translate(g1, deltax, deltay, deltaz)",
    argument(name = "g1", description = "geometry"),
    argument(name = "deltax", description = "float8"),
    argument(name = "deltay", description = "float8"),
    argument(name = "deltaz", description = "float8, default 0"),
    related_udf(name = "st_affine"),
    related_udf(name = "st_transscale")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Translate;

impl Translate {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Translate {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Translate {
    fn name(&self) -> &str {
        "st_translate"
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
        Ok(translate_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn translate_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = TranslateKernel {
        deltax: optional_float_arg(&args, 1, 0.0)?,
        deltay: optional_float_arg(&args, 2, 0.0)?,
        deltaz: optional_float_arg(&args, 3, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct TranslateKernel {
    deltax: Float64Array,
    deltay: Float64Array,
    deltaz: Float64Array,
}

impl GeometryKernel for TranslateKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        if self.deltax.is_null(row) || self.deltay.is_null(row) || self.deltaz.is_null(row) {
            return Ok(None);
        }
        // PostGIS implements ST_Translate as ST_Affine, so use the same matrix to get the same
        // floating-point results.
        let affine = Affine3D::translate(
            self.deltax.value(row),
            self.deltay.value(row),
            self.deltaz.value(row),
        );
        Ok(Some(map_coords(geom, &|c| affine.apply(c))))
    }
}
