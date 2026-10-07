//! ST_UnaryUnion.

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
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{empty_like, from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_UnaryUnion(geometry geom, float8 gridSize = -1).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "gridSize"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Unions the components of a single geometry.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Computes the union of the components of a single geometry, dissolving the overlaps of a collection or multi-geometry. If the optional gridSize argument is given (and not negative), the input is snapped to a grid of that size and the result is computed on it. This function keeps Z and drops M.",
    syntax_example = "ST_UnaryUnion(geom, gridSize)",
    argument(name = "geom", description = "geometry"),
    argument(name = "gridSize", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct UnaryUnion;

impl UnaryUnion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnaryUnion {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for UnaryUnion {
    fn name(&self) -> &str {
        "st_unaryunion"
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
        Ok(unary_union_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn unary_union_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = UnaryUnionKernel {
        // A negative grid size means none, as in PostGIS.
        grid_size: optional_float_arg(&args, 1, -1.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct UnaryUnionKernel {
    grid_size: Float64Array,
}

impl GeometryKernel for UnaryUnionKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_UnaryUnion is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.grid_size.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns EMPTY input unchanged, M included.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        let geom = to_geos(geom)?;
        let grid_size = self.grid_size.value(row);
        let result = if grid_size >= 0.0 {
            geom.unary_union_prec(grid_size)?
        } else {
            geom.unary_union()?
        };
        Ok(Some(from_geos(&result)?))
    }
}
