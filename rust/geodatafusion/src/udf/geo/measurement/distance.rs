use std::sync::{Arc, LazyLock};

use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Distance(geometry g1, geometry g2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the distance between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "For geometry types returns the minimum 2D Cartesian (planar) distance between two geometries, in projected units (spatial ref units).",
    syntax_example = "ST_Distance(g1, g2)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance;

impl Distance {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Distance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Distance {
    fn name(&self) -> &str {
        "st_distance"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let left = geometry_array(&args, 0)?;
    let right = geometry_array(&args, 1)?;
    let result = geoarrow_expr_geo::euclidean_distance(&left, &right)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

#[cfg(test)]
mod test {
    use approx::assert_relative_eq;
    use arrow_array::cast::AsArray;
    use arrow_array::types::Float64Type;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test_distance() {
        let ctx = SessionContext::new();

        ctx.register_udf(Distance::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_Distance(ST_GeomFromText('POINT(-72.1235 42.3521)'), ST_GeomFromText('LINESTRING(-72.1260 42.45, -72.123 42.1546)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        assert_relative_eq!(
            col.as_primitive::<Float64Type>().value(0),
            0.00150567726382282
        );
    }
}
