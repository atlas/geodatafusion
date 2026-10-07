//! ST_Centroid.

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
use crate::udf::geos::util::{empty_point_like, from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Centroid(geometry g1).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the geometric centre of a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a point which is the geometric centre of mass of a geometry: of its area for polygonal geometries, its length for lineal ones, and its points for puntal ones; a collection's centroid comes from its highest-dimension members. An empty input gives an empty point. The result is 2D.",
    syntax_example = "ST_Centroid(g1)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Centroid;

impl Centroid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Centroid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Centroid {
    fn name(&self) -> &str {
        "st_centroid"
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
        Ok(centroid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn centroid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = CentroidKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct CentroidKernel;

impl GeometryKernel for CentroidKernel {
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
        // The result is 2D in PostGIS.
        Ok(Some(from_geos(&to_geos(geom)?.get_centroid()?, false)?))
    }
}
