//! ST_IsValidReason.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geos::Geom;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{to_geos, valid_detail};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_int_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_IsValidReason(geometry geomA) and ST_IsValidReason(geometry geomA, integer flags).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geomA", "flags"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Says why a geometry is invalid.
#[user_doc(
    doc_section(label = "Geometry Validation"),
    description = "Returns text stating whether a geometry is valid, or, if invalid, the reason and the location of the problem, such as Self-intersection[5 5]. An empty geometry is valid. With flags, as in PostGIS, the reason comes without the location; flag 1 lets self-touching rings form holes, as ESRI's rules do.",
    syntax_example = "ST_IsValidReason(geomA, flags)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "flags", description = "integer"),
    related_udf(name = "st_isvalid"),
    related_udf(name = "st_makevalid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValidReason;

impl IsValidReason {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsValidReason {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValidReason {
    fn name(&self) -> &str {
        "st_isvalidreason"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_valid_reason_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_valid_reason_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: StringArray = if args.args.len() > 1 {
        let kernel = FlagsKernel {
            flags: optional_int_arg(&args, 1, 0)?,
        };
        map_geometry(geometries.as_ref(), &kernel)?
    } else {
        map_geometry(geometries.as_ref(), &IsValidReasonKernel)?
    };
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// The reason with flags: GEOS's validity detail, without the location.
struct FlagsKernel {
    flags: Int32Array,
}

impl GeometryKernel for FlagsKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        if self.flags.is_null(row) {
            return Ok(None);
        }
        let detail = valid_detail(geom, self.flags.value(row))
            .map_err(|e| exec_datafusion_err!("st_isvalidreason: {e}"))?;
        Ok(Some(match detail.reason {
            Some(reason) if !detail.valid => reason,
            _ => "Valid Geometry".to_string(),
        }))
    }
}

struct IsValidReasonKernel;

impl GeometryKernel for IsValidReasonKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // An EMPTY geometry is valid in PostGIS.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some("Valid Geometry".to_string()));
        }
        Ok(Some(to_geos(geom)?.is_valid_reason()?))
    }
}
