//! ST_Snap.

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
use crate::udf::geos::util::{GeosColumn, from_geos, has_z, to_geos};
use crate::util::args::optional_float_arg;
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Snap(geometry geom1, geometry geom2, float8 tolerance).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2", "tolerance"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Snaps the vertices and segments of a geometry to another's vertices.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Snaps the vertices and segments of geom1 to the vertices of geom2 within a snap distance tolerance. Snapping can make coincident edges exactly coincident, for overlay. Vertices snapped from a 2D geom2 have no Z value (NaN). This function keeps Z and drops M.",
    syntax_example = "ST_Snap(geom1, geom2, tolerance)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "tolerance", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Snap;

impl Snap {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Snap {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Snap {
    fn name(&self) -> &str {
        "st_snap"
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
        Ok(snap_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn snap_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata("st_snap", &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = SnapKernel {
        other: GeosColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        tolerance: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SnapKernel {
    other: GeosColumn,
    tolerance: Float64Array,
}

impl GeometryKernel for SnapKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_Snap is STRICT: SQL NULL in any argument gives SQL NULL.
        let Some((other_input, other)) = self.other.get(row) else {
            return Ok(None);
        };
        // PostGIS keeps a Z from GEOS only when an input has Z.
        let want_z = has_z(geom) || has_z(other_input);
        if self.tolerance.is_null(row) {
            return Ok(None);
        }
        let snapped = to_geos(geom)?.snap(other, self.tolerance.value(row))?;
        Ok(Some(from_geos(&snapped, want_z)?))
    }
}
