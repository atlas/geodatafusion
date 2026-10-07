use std::sync::Arc;

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundsKernel;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the X minimum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry.",
    syntax_example = "ST_XMin(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_ymin"),
    related_udf(name = "st_zmin"),
    related_udf(name = "st_xmax"),
    related_udf(name = "st_ymax"),
    related_udf(name = "st_zmax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct XMin;

impl XMin {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for XMin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for XMin {
    fn name(&self) -> &str {
        "st_xmin"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::XMin)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the Y minimum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry.",
    syntax_example = "ST_YMin(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_xmin"),
    related_udf(name = "st_zmin"),
    related_udf(name = "st_xmax"),
    related_udf(name = "st_ymax"),
    related_udf(name = "st_zmax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct YMin;

impl YMin {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for YMin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for YMin {
    fn name(&self) -> &str {
        "st_ymin"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::YMin)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the Z minimum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry. The Z of a geometry without Z is 0.",
    syntax_example = "ST_ZMin(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_xmin"),
    related_udf(name = "st_ymin"),
    related_udf(name = "st_xmax"),
    related_udf(name = "st_ymax"),
    related_udf(name = "st_zmax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ZMin;

impl ZMin {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ZMin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ZMin {
    fn name(&self) -> &str {
        "st_zmin"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::ZMin)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the X maximum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry.",
    syntax_example = "ST_XMax(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_xmin"),
    related_udf(name = "st_ymin"),
    related_udf(name = "st_zmin"),
    related_udf(name = "st_ymax"),
    related_udf(name = "st_zmax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct XMax;

impl XMax {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for XMax {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for XMax {
    fn name(&self) -> &str {
        "st_xmax"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::XMax)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the Y maximum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry.",
    syntax_example = "ST_YMax(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_xmin"),
    related_udf(name = "st_ymin"),
    related_udf(name = "st_zmin"),
    related_udf(name = "st_xmax"),
    related_udf(name = "st_zmax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct YMax;

impl YMax {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for YMax {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for YMax {
    fn name(&self) -> &str {
        "st_ymax"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::YMax)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the Z maximum of a 2D or 3D bounding box or a geometry. Returns NULL for an empty geometry. The Z of a geometry without Z is 0.",
    syntax_example = "ST_ZMax(aGeomorBox2DorBox3D)",
    argument(name = "aGeomorBox2DorBox3D", description = "box3d"),
    related_udf(name = "st_xmin"),
    related_udf(name = "st_ymin"),
    related_udf(name = "st_zmin"),
    related_udf(name = "st_xmax"),
    related_udf(name = "st_ymax")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ZMax;

impl ZMax {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ZMax {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ZMax {
    fn name(&self) -> &str {
        "st_zmax"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(extrema_impl(args, Extremum::ZMax)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn extrema_impl(
    args: ScalarFunctionArgs,
    extremum: Extremum,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &extremum)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

#[derive(Debug, Clone, Copy)]
enum Extremum {
    XMin,
    YMin,
    ZMin,
    XMax,
    YMax,
    ZMax,
}

impl GeometryKernel for Extremum {
    type Output = f64;

    /// NULL for an EMPTY geometry; the Z of a geometry without Z is 0, as in PostGIS.
    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let kernel = BoundsKernel { include_z: true };
        Ok(kernel.eval(geom, row)?.map(|rect| match self {
            Extremum::XMin => rect.minx(),
            Extremum::YMin => rect.miny(),
            Extremum::ZMin => rect.minz(),
            Extremum::XMax => rect.maxx(),
            Extremum::YMax => rect.maxy(),
            Extremum::ZMax => rect.maxz(),
        }))
    }
}

#[cfg(test)]
mod test {
    use approx::relative_eq;
    use arrow_array::cast::AsArray;
    use arrow_array::types::Float64Type;
    use datafusion::prelude::*;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    async fn extrema_test(udf: &str, expected: f64) {
        let ctx = SessionContext::new();

        ctx.register_udf(XMin::new().into());
        ctx.register_udf(YMin::new().into());
        ctx.register_udf(ZMin::new().into());
        ctx.register_udf(XMax::new().into());
        ctx.register_udf(YMax::new().into());
        ctx.register_udf(ZMax::new().into());
        ctx.register_udf(GeomFromText::default().into());

        let out = ctx
            .sql(&format!(
                "SELECT {udf}(ST_GeomFromText('LINESTRING Z(1 2 3, 3 4 5, 5 6 7)'));"
            ))
            .await
            .unwrap();
        let batch = out.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let arr = col.as_primitive::<Float64Type>();
        assert!(relative_eq!(arr.value(0), expected));
    }

    #[tokio::test]
    async fn test_2d() {
        extrema_test("ST_XMin", 1.0).await;
        extrema_test("ST_YMin", 2.0).await;
        extrema_test("ST_ZMin", 3.0).await;
        extrema_test("ST_XMax", 5.0).await;
        extrema_test("ST_YMax", 6.0).await;
        extrema_test("ST_ZMax", 7.0).await;
    }
}
