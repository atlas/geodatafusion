use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, Metadata};

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::twkb;
use crate::util::field::wkb_return_field;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_GeomFromTWKB(bytea twkb).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Bytea]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["twkb"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a geometry instance from a TWKB ("Tiny Well-Known Binary") geometry representation.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the geometry a Tiny Well-Known Binary (TWKB) value describes, without SRID. As in PostGIS, sizes, bounding boxes, ID lists and bytes after the geometry are skipped unchecked, and an unclosed polygon ring is closed; a line needs at least two points and a ring four.",
    syntax_example = "ST_GeomFromTWKB(twkb)",
    argument(name = "twkb", description = "bytea"),
    related_udf(name = "st_astwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromTWKB;

impl GeomFromTWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomFromTWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromTWKB {
    fn name(&self) -> &str {
        "st_geomfromtwkb"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Arc::new(Metadata::default())))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_twkb_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geom_from_twkb_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(internal_datafusion_err!("{name}: unexpected return field").into());
    };
    let buffers = args.args[0]
        .cast_to(&DataType::Binary, None)?
        .to_array(args.number_rows)?;
    let mut builder = WkbBuilder::<i32>::new(output_type);
    for buffer in buffers.as_binary::<i32>() {
        let Some(buffer) = buffer else {
            builder.push_geometry(None::<&wkt::Wkt<f64>>)?;
            continue;
        };
        let geometry =
            twkb::read(buffer).map_err(|message| exec_datafusion_err!("{name}: {message}"))?;
        builder.push_geometry(Some(&geometry))?;
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}
