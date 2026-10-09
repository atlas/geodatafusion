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
use geo_traits::{GeometryTrait, GeometryType, LineStringTrait};
use wkt::types::Point;

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_coord};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_PointN(geometry a_linestring, integer n).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_linestring", "n"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the Nth point in the first LineString or circular LineString in a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the nth point of a LINESTRING. n is 1-based, and a negative n counts back from the end, -1 being the last point. Returns NULL if n is 0 or out of range, and for any other geometry type.",
    syntax_example = "ST_PointN(a_linestring, n)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "n", description = "integer"),
    related_udf(name = "st_numpoints"),
    related_udf(name = "st_startpoint"),
    related_udf(name = "st_endpoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointN;

impl PointN {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointN {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointN {
    fn name(&self) -> &str {
        "st_pointn"
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
        Ok(point_n_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn point_n_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = PointNKernel {
        n: optional_int_arg(&args, 1, 1)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PointNKernel {
    n: Int32Array,
}

impl GeometryKernel for PointNKernel {
    type Output = Point<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Point<f64>>> {
        if self.n.is_null(row) {
            return Ok(None);
        }
        let GeometryType::LineString(line) = geom.as_type() else {
            return Ok(None);
        };
        let n = i64::from(self.n.value(row));
        let count = line.num_coords() as i64;
        // 1-based from the start, -1-based from the end; 0 is out of range.
        let index = match n {
            1.. => n - 1,
            ..0 => count + n,
            0 => return Ok(None),
        };
        let Some(coord) = usize::try_from(index).ok().and_then(|i| line.coord(i)) else {
            return Ok(None);
        };
        Ok(Some(Point::new(
            Some(owned_coord(&coord)),
            dimension(geom.dim()),
        )))
    }
}
