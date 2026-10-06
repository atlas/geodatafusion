//! Point ordinate accessors: ST_X, ST_Y, ST_Z and ST_M.

use std::sync::Arc;

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::{m, z};
use crate::util::signature::single_geometry;

/// Returns the X coordinate of a Point.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the X coordinate of a Point. An EMPTY point gives NULL; any other geometry type is an error.",
    syntax_example = "ST_X(a_point)",
    argument(name = "a_point", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct X;

impl X {
    pub fn new() -> Self {
        Self
    }
}

impl Default for X {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for X {
    fn name(&self) -> &str {
        "st_x"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(ordinate_impl(self.name(), args, Ordinate::X)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the Y coordinate of a Point.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the Y coordinate of a Point. An EMPTY point gives NULL; any other geometry type is an error.",
    syntax_example = "ST_Y(a_point)",
    argument(name = "a_point", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Y;

impl Y {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Y {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Y {
    fn name(&self) -> &str {
        "st_y"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(ordinate_impl(self.name(), args, Ordinate::Y)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the Z coordinate of a Point.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the Z coordinate of a Point, or NULL if it has none. An EMPTY point gives NULL; any other geometry type is an error.",
    syntax_example = "ST_Z(a_point)",
    argument(name = "a_point", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Z;

impl Z {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Z {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Z {
    fn name(&self) -> &str {
        "st_z"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(ordinate_impl(self.name(), args, Ordinate::Z)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the M coordinate of a Point.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the M coordinate of a Point, or NULL if it has none. An EMPTY point gives NULL; any other geometry type is an error.",
    syntax_example = "ST_M(a_point)",
    argument(name = "a_point", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct M;

impl M {
    pub fn new() -> Self {
        Self
    }
}

impl Default for M {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for M {
    fn name(&self) -> &str {
        "st_m"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(ordinate_impl(self.name(), args, Ordinate::M)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum Ordinate {
    X,
    Y,
    Z,
    M,
}

fn ordinate_impl(
    name: &str,
    args: ScalarFunctionArgs,
    ordinate: Ordinate,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array =
        map_geometry(geometries.as_ref(), &OrdinateKernel { name, ordinate })?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct OrdinateKernel<'a> {
    name: &'a str,
    ordinate: Ordinate,
}

impl GeometryKernel for OrdinateKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let GeometryType::Point(point) = geom.as_type() else {
            return Err(
                exec_datafusion_err!("{}: Argument must have type POINT", self.name).into(),
            );
        };
        // An EMPTY point has no coordinate, or NaN coordinates in WKB.
        let Some(coord) = point
            .coord()
            .filter(|c| !(c.x().is_nan() && c.y().is_nan()))
        else {
            return Ok(None);
        };
        Ok(match self.ordinate {
            Ordinate::X => Some(coord.x()),
            Ordinate::Y => Some(coord.y()),
            Ordinate::Z => z(&coord),
            Ordinate::M => m(&coord),
        })
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    /// The planned schema matches the computed one, including nullability.
    #[tokio::test]
    async fn test_return_schema() {
        let ctx = SessionContext::new();
        ctx.register_udf(X.into());
        ctx.register_udf(Y.into());
        ctx.register_udf(Z.into());
        ctx.register_udf(M.into());
        ctx.register_udf(GeomFromText::new(Default::default()).into());

        for function in ["ST_X", "ST_Y", "ST_Z", "ST_M"] {
            // A column, so the call isn't constant-folded.
            let sql = format!(
                "SELECT {function}(ST_GeomFromText(t)) \
                 FROM (VALUES ('POINT ZM (1 2 3 4)'), (NULL)) AS v(t)"
            );
            let df = ctx.sql(&sql).await.unwrap();
            let df_schema = df.schema().inner().clone();
            let batch = df.collect().await.unwrap().into_iter().next().unwrap();
            assert_eq!(df_schema, batch.schema(), "{sql}");
            assert!(df_schema.field(0).is_nullable(), "{sql}");
        }
    }
}
