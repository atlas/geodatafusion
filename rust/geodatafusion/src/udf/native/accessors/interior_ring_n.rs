use std::sync::LazyLock;

use arrow_array::{Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, PolygonTrait};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::line_string_to_owned;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_InteriorRingN(geometry a_polygon, integer n).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_polygon", "n"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the Nth interior ring (hole) of a Polygon.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the 1-based nth interior ring of a POLYGON as a LINESTRING. Returns NULL if n is out of range, and for any other geometry type, including a MULTIPOLYGON.",
    syntax_example = "ST_InteriorRingN(a_polygon, n)",
    argument(name = "a_polygon", description = "geometry"),
    argument(name = "n", description = "integer"),
    related_udf(name = "st_exteriorring"),
    related_udf(name = "st_numinteriorrings")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct InteriorRingN;

impl InteriorRingN {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InteriorRingN {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for InteriorRingN {
    fn name(&self) -> &str {
        "st_interiorringn"
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
        Ok(interior_ring_n_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn interior_ring_n_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = InteriorRingNKernel {
        n: optional_int_arg(&args, 1, 1)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct InteriorRingNKernel {
    n: Int32Array,
}

impl GeometryKernel for InteriorRingNKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.n.is_null(row) {
            return Ok(None);
        }
        let GeometryType::Polygon(polygon) = geom.as_type() else {
            return Ok(None);
        };
        // 1-based; zero and negative numbers are out of range.
        let Some(index) = usize::try_from(self.n.value(row))
            .ok()
            .and_then(|n| n.checked_sub(1))
        else {
            return Ok(None);
        };
        Ok(polygon
            .interior(index)
            .map(|ring| line_string_to_owned(&ring, geom.dim())))
    }
}
