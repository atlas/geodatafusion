use std::sync::Arc;

use arrow_array::Int16Array;
use arrow_schema::DataType;
use datafusion::common::internal_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{Dimensions, GeometryTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the coordinate dimension of the geometry: the number of ordinates of its coordinates (2 for XY, 3 for XYZ and XYM, 4 for XYZM). The same as ST_NDims.",
    syntax_example = "ST_CoordDim(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_ndims")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CoordDim;

impl CoordDim {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for CoordDim {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for CoordDim {
    fn name(&self) -> &str {
        "st_coorddim"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int16)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(coord_dim_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn coord_dim_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int16Array = map_geometry(geometries.as_ref(), &CoordDimKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// The number of ordinates of a geometry: 2, 3 or 4. Read per row, because a column in a
/// mixed-dimension encoding can hold every dimension.
struct CoordDimKernel;

impl GeometryKernel for CoordDimKernel {
    type Output = i16;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i16>> {
        let size = match geom.dim() {
            Dimensions::Xy => 2,
            Dimensions::Xyz | Dimensions::Xym => 3,
            Dimensions::Xyzm => 4,
            Dimensions::Unknown(size) => i16::try_from(size).map_err(|_| {
                internal_datafusion_err!("st_coorddim: unexpected dimension {size}")
            })?,
        };
        Ok(Some(size))
    }
}

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the coordinate dimension of the geometry: the number of ordinates of its coordinates (2 for XY, 3 for XYZ and XYM, 4 for XYZM).",
    syntax_example = "ST_NDims(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_coorddim")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct NDims;

impl NDims {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for NDims {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for NDims {
    fn name(&self) -> &str {
        "st_ndims"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int16)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(coord_dim_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int16Type;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test_coord_dim() {
        let ctx = SessionContext::new();

        ctx.register_udf(CoordDim::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_CoordDim(ST_GeomFromText('POINT(1 1)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Int16Type>().value(0);
        assert_eq!(val, 2);
    }

    #[tokio::test]
    async fn test_ndims() {
        let ctx = SessionContext::new();

        ctx.register_udf(NDims::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_NDims(ST_GeomFromText('POINT(1 1)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_primitive::<Int16Type>().value(0);
        assert_eq!(val, 2);
    }
}
