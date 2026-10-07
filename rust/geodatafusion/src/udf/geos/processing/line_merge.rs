use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray};
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
use crate::udf::geos::util::{empty_like, from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_bool_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineMerge(geometry amultilinestring) and
/// ST_LineMerge(geometry amultilinestring, boolean directed).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Boolean]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Sews together the component lines of a (multi)linestring.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a (set of) LineString(s) formed by sewing together the constituent line work of a MultiLineString. Lines are joined at endpoints where exactly two lines meet; lines are not merged across intersections of three or more lines. When `directed` is true, lines are only merged when their directions agree. Non-linear inputs yield an empty GeometryCollection, polygons their rings, and empty inputs are returned unchanged. This function keeps Z and drops M.",
    syntax_example = "ST_LineMerge(amultilinestring, directed)",
    argument(name = "amultilinestring", description = "geometry"),
    argument(name = "directed", description = "boolean")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineMerge;

impl LineMerge {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineMerge {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineMerge {
    fn name(&self) -> &str {
        "st_linemerge"
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
        Ok(line_merge_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Parse the optional `directed` argument.
///
/// Absent or null is treated as `false`.
fn line_merge_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineMergeKernel {
        directed: optional_bool_arg(&args, 1, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineMergeKernel {
    directed: BooleanArray,
}

impl GeometryKernel for LineMergeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_LineMerge is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.directed.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns EMPTY input unchanged, M included; GEOS would return an empty
        // collection.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        let geom = to_geos(geom)?;
        let merged = if self.directed.value(row) {
            geom.line_merge_directed()?
        } else {
            geom.line_merge()?
        };
        Ok(Some(from_geos(&merged)?))
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_line_merge_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(LineMerge.into());
        ctx.register_udf(GeomFromText::new().into());

        let sql =
            "SELECT ST_LineMerge(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(1 1,2 2))', 4326))";
        assert_wkb_output(&ctx, sql, 4326).await;
    }
}
