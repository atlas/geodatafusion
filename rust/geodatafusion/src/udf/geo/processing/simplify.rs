use std::sync::{Arc, LazyLock};

use arrow_array::new_null_array;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{internal_err, not_impl_datafusion_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion::scalar::ScalarValue;
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_schema::error::GeoArrowResult;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_result, wkb_return_field};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Simplify(geometry geom, float tolerance) and the same for ST_SimplifyVW.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "tolerance"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a simplified representation of a geometry, using the Douglas-Peucker algorithm.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a simplified representation of a geometry using the Douglas-Peucker algorithm. The simplification tolerance is a distance value, in the units of the input SRS. Simplification removes vertices which are within the tolerance distance of the simplified linework. The result may not be valid even if the input is. Unlike PostGIS, the tolerance must be a constant.",
    syntax_example = "ST_Simplify(geom, tolerance)",
    argument(name = "geom", description = "geometry"),
    argument(name = "tolerance", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Simplify;

impl Simplify {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Simplify {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Simplify {
    fn name(&self) -> &str {
        "st_simplify"
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
        Ok(simplify_impl(
            self.name(),
            args,
            geoarrow_expr_geo::simplify,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a simplified representation of a geometry, using the Visvalingam-Whyatt algorithm.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a simplified representation of a geometry using the Visvalingam-Whyatt algorithm. The simplification tolerance is an area value, in the units of the input SRS. Simplification removes vertices which form \"corners\" with area less than the tolerance. The result may not be valid even if the input is. Unlike PostGIS, the tolerance must be a constant.",
    syntax_example = "ST_SimplifyVW(geom, tolerance)",
    argument(name = "geom", description = "geometry"),
    argument(name = "tolerance", description = "float8")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SimplifyVW;

impl SimplifyVW {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimplifyVW {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SimplifyVW {
    fn name(&self) -> &str {
        "st_simplifyvw"
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
        Ok(simplify_impl(
            self.name(),
            args,
            geoarrow_expr_geo::simplify_vw,
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a simplified and valid representation of a geometry.
fn simplify_impl(
    name: &str,
    args: ScalarFunctionArgs,
    simplify_fn: impl Fn(&dyn GeoArrowArray, f64) -> GeoArrowResult<Arc<dyn GeoArrowArray>>,
) -> GeoDataFusionResult<ColumnarValue> {
    let tolerance = match &args.args[1] {
        ColumnarValue::Scalar(ScalarValue::Float64(tolerance)) => *tolerance,
        // The geoarrow-expr-geo kernels take one tolerance for the whole array.
        _ => {
            return Err(
                not_impl_datafusion_err!("{name} only supports a constant tolerance").into(),
            );
        }
    };
    // SQL NULL in, SQL NULL out.
    let Some(tolerance) = tolerance else {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    };
    let geometries = geometry_array(&args, 0)?;
    // geoarrow-expr-geo builds a native array; the result is converted to WKB.
    let result = simplify_fn(&geometries, tolerance)?;
    wkb_result(result.as_ref(), &args.return_field)
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use datafusion::prelude::*;

    use super::*;
    use crate::udf::native::io::{AsText, GeomFromText};
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_simplify_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(Simplify.into());
        ctx.register_udf(GeomFromText::default().into());

        let sql =
            "SELECT ST_Simplify(ST_GeomFromText('LINESTRING(0 0,5 4,11 5.5,27.8 0.1)', 3857), 1.0)";
        assert_wkb_output(&ctx, sql, 3857).await;
    }

    #[tokio::test]
    async fn test_simplify_vw() {
        let ctx = SessionContext::new();

        ctx.register_udf(SimplifyVW.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(AsText.into());

        let df = ctx.sql(
            "SELECT ST_AsText(ST_SimplifyVW(ST_GeomFromText('LINESTRING(5 2, 3 8, 6 20, 7 25, 10 10)'), 30));").await.unwrap();
        let batches = df.collect().await.unwrap();
        let batch = batches.first().unwrap();
        let column = batch.column(0);
        let val = column.as_string::<i32>().value(0);
        assert_eq!(val, "LINESTRING(5 2,7 25,10 10)");
    }
}
