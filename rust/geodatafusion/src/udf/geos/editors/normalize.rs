//! ST_Normalize.

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
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Normalize(geometry geom).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Puts a geometry in its canonical form.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry in its normalized (canonical) form: vertices and parts are ordered so that equal geometries compare equal. This function keeps Z and drops M.",
    syntax_example = "ST_Normalize(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Normalize;

impl Normalize {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Normalize {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Normalize {
    fn name(&self) -> &str {
        "st_normalize"
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
        Ok(normalize_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn normalize_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = NormalizeKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct NormalizeKernel;

impl GeometryKernel for NormalizeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // Unlike most GEOS-backed PostGIS functions, ST_Normalize doesn't return EMPTY input
        // unchanged: an EMPTY input loses M too.
        let mut normalized = to_geos(geom)?;
        normalized.normalize()?;
        Ok(Some(from_geos(&normalized)?))
    }
}
