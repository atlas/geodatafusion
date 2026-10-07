//! ST_PointOnSurface.

use std::sync::LazyLock;

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
use crate::udf::geos::util::{empty_point_like, from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_PointOnSurface(geometry g1).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a point guaranteed to lie on a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a point guaranteed to lie in the interior of a polygonal geometry, on a lineal one, or at one of the points of a puntal one. An empty input gives an empty point. This function keeps Z where GEOS keeps it, and drops M.",
    syntax_example = "ST_PointOnSurface(g1)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointOnSurface;

impl PointOnSurface {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointOnSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointOnSurface {
    fn name(&self) -> &str {
        "st_pointonsurface"
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
        Ok(point_on_surface_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn point_on_surface_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = PointOnSurfaceKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PointOnSurfaceKernel;

impl GeometryKernel for PointOnSurfaceKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS returns an empty point, with the input's dimensions, for EMPTY input.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_point_like(geom)));
        }
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        Ok(Some(from_geos(
            &to_geos(geom)?.point_on_surface()?,
            want_z,
        )?))
    }
}
