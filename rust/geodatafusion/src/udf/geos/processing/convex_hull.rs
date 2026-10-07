//! ST_ConvexHull.

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
use crate::udf::geos::util::{empty_like, from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ConvexHull(geometry geomA).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geomA"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the convex hull of a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes the convex hull of a geometry: the smallest convex geometry that encloses all its points. The result is a polygon, or a linestring or point for collinear or single-point input. An empty input is returned unchanged. This function keeps Z and drops M.",
    syntax_example = "ST_ConvexHull(geomA)",
    argument(name = "geomA", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ConvexHull;

impl ConvexHull {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConvexHull {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ConvexHull {
    fn name(&self) -> &str {
        "st_convexhull"
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
        Ok(convex_hull_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn convex_hull_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ConvexHullKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ConvexHullKernel;

impl GeometryKernel for ConvexHullKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS returns EMPTY input unchanged, M included.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        Ok(Some(from_geos(&to_geos(geom)?.convex_hull()?, want_z)?))
    }
}
