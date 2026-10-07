//! ST_ReducePrecision.

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
use geos::{Geom, Precision};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{empty_like, from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ReducePrecision(geometry g, float8 gridsize).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g", "gridsize"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Snaps a geometry to a grid, keeping it valid.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a valid geometry with all points rounded to the provided grid tolerance, and features below the tolerance removed. Unlike ST_SnapToGrid the result is valid, with no ring self-intersections or collapsed components. A grid size of 0 keeps the precision. This function keeps Z and drops M.",
    syntax_example = "ST_ReducePrecision(g, gridsize)",
    argument(name = "g", description = "geometry"),
    argument(name = "gridsize", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ReducePrecision;

impl ReducePrecision {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReducePrecision {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ReducePrecision {
    fn name(&self) -> &str {
        "st_reduceprecision"
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
        Ok(reduce_precision_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn reduce_precision_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ReducePrecisionKernel {
        grid_size: optional_float_arg(&args, 1, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ReducePrecisionKernel {
    grid_size: Float64Array,
}

impl GeometryKernel for ReducePrecisionKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        // ST_ReducePrecision is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.grid_size.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns EMPTY input unchanged, M included.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        let grid_size = self.grid_size.value(row);
        let reduced = to_geos(geom)?.set_precision(grid_size, Precision::ValidOutput)?;
        Ok(Some(from_geos(&reduced, want_z)?))
    }
}
