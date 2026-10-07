//! ST_OrientedEnvelope.

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
use wkt::types::{Dimension, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_OrientedEnvelope(geometry geom).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the minimum-area rotated rectangle enclosing a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns the minimum-area rotated rectangle enclosing a geometry. The result is a polygon, or a linestring or point for degenerate input. An empty input gives an empty polygon. The result is 2D.",
    syntax_example = "ST_OrientedEnvelope(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct OrientedEnvelope;

impl OrientedEnvelope {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OrientedEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for OrientedEnvelope {
    fn name(&self) -> &str {
        "st_orientedenvelope"
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
        Ok(oriented_envelope_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn oriented_envelope_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = OrientedEnvelopeKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct OrientedEnvelopeKernel;

impl GeometryKernel for OrientedEnvelopeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS returns an empty polygon for EMPTY input.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(Wkt::Polygon(Polygon::empty(Dimension::XY))));
        }
        // The result is 2D in PostGIS.
        Ok(Some(from_geos(
            &to_geos(geom)?.minimum_rotated_rectangle()?,
            false,
        )?))
    }
}
