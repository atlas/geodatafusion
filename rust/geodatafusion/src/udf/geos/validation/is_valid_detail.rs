//! ST_IsValidDetail.

use std::sync::{Arc, LazyLock};

use arrow_array::builder::{BooleanBuilder, StringBuilder};
use arrow_array::{Array, ArrayRef, Int32Array, StructArray};
use arrow_buffer::NullBufferBuilder;
use arrow_schema::{DataType, Field, FieldRef, Fields};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::WkbType;
use wkt::types::{Coord, Dimension, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{ValidDetail, valid_detail};
use crate::util::args::optional_int_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_IsValidDetail(geometry geom, integer flags = 0).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "flags"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a valid_detail row stating if a geometry is valid or if not a reason and a location.
#[user_doc(
    doc_section(label = "Geometry Validation"),
    description = "Returns PostGIS's valid_detail record for a geometry, as a struct: valid, whether it is valid; reason, why not; and location, a point where the problem is, without SRID. A valid geometry has NULL reason and location. Flag 1 lets self-touching rings form holes, as ESRI's rules do. In FROM, st_isvaliddetail(...) (in lowercase) gives the record as one row.",
    syntax_example = "ST_IsValidDetail(geom, flags)",
    argument(name = "geom", description = "geometry"),
    argument(name = "flags", description = "integer, default 0")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsValidDetail;

impl IsValidDetail {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsValidDetail {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsValidDetail {
    fn name(&self) -> &str {
        "st_isvaliddetail"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(Arc::new(Field::new(
            self.name(),
            DataType::Struct(record_fields()),
            true,
        )))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_valid_detail_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The fields of PostGIS's `valid_detail` record. The location has no CRS: PostGIS gives it no
/// SRID.
fn record_fields() -> Fields {
    let location = Field::new("location", DataType::Binary, true)
        .with_extension_type(WkbType::new(Default::default()));
    Fields::from(vec![
        Field::new("valid", DataType::Boolean, true),
        Field::new("reason", DataType::Utf8, true),
        location,
    ])
}

fn is_valid_detail_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = DetailKernel {
        flags: optional_int_arg(&args, 1, 0)?,
    };
    let details: Vec<Option<ValidDetail>> = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(record_array(&details)?))
}

struct DetailKernel {
    flags: Int32Array,
}

impl GeometryKernel for DetailKernel {
    type Output = ValidDetail;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<ValidDetail>> {
        if self.flags.is_null(row) {
            return Ok(None);
        }
        let detail = valid_detail(geom, self.flags.value(row))
            .map_err(|e| exec_datafusion_err!("st_isvaliddetail: {e}"))?;
        Ok(Some(detail))
    }
}

/// The records of the rows, NULL where a row is `None`.
fn record_array(details: &[Option<ValidDetail>]) -> GeoDataFusionResult<ArrayRef> {
    let mut valid = BooleanBuilder::with_capacity(details.len());
    let mut reason = StringBuilder::new();
    let mut location = WkbBuilder::<i32>::new(WkbType::new(Default::default()));
    let mut nulls = NullBufferBuilder::new(details.len());
    for detail in details {
        match detail {
            None => {
                valid.append_null();
                reason.append_null();
                location.push_geometry(None::<&Point<f64>>)?;
                nulls.append_null();
            }
            Some(detail) => {
                valid.append_value(detail.valid);
                reason.append_option(detail.reason.as_deref());
                let point = detail.location.map(|(x, y)| {
                    Point::new(
                        Some(Coord {
                            x,
                            y,
                            z: None,
                            m: None,
                        }),
                        Dimension::XY,
                    )
                });
                location.push_geometry(point.as_ref())?;
                nulls.append_non_null();
            }
        }
    }
    let columns: Vec<ArrayRef> = vec![
        Arc::new(valid.finish()),
        Arc::new(reason.finish()),
        location.finish().into_array_ref(),
    ];
    Ok(Arc::new(StructArray::try_new(
        record_fields(),
        columns,
        nulls.finish(),
    )?))
}
