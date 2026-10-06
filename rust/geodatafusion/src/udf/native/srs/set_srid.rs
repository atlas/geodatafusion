use std::sync::{Arc, LazyLock};

use arrow_array::BinaryArray;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::WkbArray;
use geoarrow_array::cast::to_wkb;
use geoarrow_schema::Metadata;

use crate::error::GeoDataFusionResult;
use crate::util::args::scalar_srid;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_SetSRID(geometry geom, integer srid).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Srid]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Sets the SRID on a geometry.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Sets the SRID on a geometry to a particular integer value, without transforming its coordinates. Unlike PostGIS, the SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_SetSRID(geom, srid)",
    argument(name = "geom", description = "geometry"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_srid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub struct SetSRID;

impl SetSRID {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SetSRID {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SetSRID {
    fn name(&self) -> &str {
        "st_setsrid"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let edges = input_metadata(&args.arg_fields[0]).edges();
        let crs = scalar_srid(self.name(), &args, 1)?
            .map(srid_to_crs)
            .unwrap_or_default();
        Ok(wkb_return_field(
            self.name(),
            Arc::new(Metadata::new(crs, edges)),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(set_srid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn set_srid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    // The coordinates are unchanged; only the CRS in the return field differs.
    let metadata = input_metadata(&args.return_field);
    // SQL NULL in, SQL NULL out.
    let srid_is_null = matches!(&args.args[1], ColumnarValue::Scalar(srid) if srid.is_null());
    let binary = if srid_is_null {
        BinaryArray::new_null(args.number_rows)
    } else {
        let geometries = geometry_array(&args, 0)?;
        to_wkb::<i32>(geometries.as_ref())?.inner().clone()
    };
    Ok(ColumnarValue::Array(
        WkbArray::new(binary, metadata).into_array_ref(),
    ))
}
