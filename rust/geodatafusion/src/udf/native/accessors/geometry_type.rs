use std::sync::Arc;

use arrow_array::StringArray;
use arrow_schema::DataType;
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
    description = "Returns the type of the geometry as a string, for example 'LINESTRING', 'POLYGON' or 'MULTIPOINT'. Geometries with M but no Z get an M suffix ('POINTM').",
    syntax_example = "GeometryType(geomA)",
    argument(name = "geomA", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeometryType;

impl GeometryType {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for GeometryType {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeometryType {
    fn name(&self) -> &str {
        "geometrytype"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geometry_type_impl(args, Style::Upper)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the type of the geometry as a string, for example 'ST_LineString', 'ST_Polygon' or 'ST_MultiPolygon'. Unlike GeometryType, the string has an ST_ prefix and no dimension suffix.",
    syntax_example = "ST_GeometryType(g1)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
#[allow(non_camel_case_types)]
pub struct ST_GeometryType;

impl ST_GeometryType {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ST_GeometryType {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ST_GeometryType {
    fn name(&self) -> &str {
        "st_geometrytype"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geometry_type_impl(args, Style::Prefixed)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geometry_type_impl(
    args: ScalarFunctionArgs,
    style: Style,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: StringArray = map_geometry(geometries.as_ref(), &style)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// How the type name is spelled.
#[derive(Debug, Clone, Copy)]
enum Style {
    /// GeometryType: `MULTILINESTRING`, with an `M` suffix for XYM geometries only.
    Upper,
    /// ST_GeometryType: `ST_MultiLineString`, without a dimension suffix.
    Prefixed,
}

impl GeometryKernel for Style {
    type Output = &'static str;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<&'static str>> {
        use geo_traits::GeometryType::*;

        let measured = geom.dim() == Dimensions::Xym;
        let name = match (self, geom.as_type()) {
            (Style::Upper, Point(_)) if measured => "POINTM",
            (Style::Upper, Point(_)) => "POINT",
            (Style::Upper, LineString(_) | Line(_)) if measured => "LINESTRINGM",
            (Style::Upper, LineString(_) | Line(_)) => "LINESTRING",
            (Style::Upper, Polygon(_) | Rect(_) | Triangle(_)) if measured => "POLYGONM",
            (Style::Upper, Polygon(_) | Rect(_) | Triangle(_)) => "POLYGON",
            (Style::Upper, MultiPoint(_)) if measured => "MULTIPOINTM",
            (Style::Upper, MultiPoint(_)) => "MULTIPOINT",
            (Style::Upper, MultiLineString(_)) if measured => "MULTILINESTRINGM",
            (Style::Upper, MultiLineString(_)) => "MULTILINESTRING",
            (Style::Upper, MultiPolygon(_)) if measured => "MULTIPOLYGONM",
            (Style::Upper, MultiPolygon(_)) => "MULTIPOLYGON",
            (Style::Upper, GeometryCollection(_)) if measured => "GEOMETRYCOLLECTIONM",
            (Style::Upper, GeometryCollection(_)) => "GEOMETRYCOLLECTION",
            (Style::Prefixed, Point(_)) => "ST_Point",
            (Style::Prefixed, LineString(_) | Line(_)) => "ST_LineString",
            (Style::Prefixed, Polygon(_) | Rect(_) | Triangle(_)) => "ST_Polygon",
            (Style::Prefixed, MultiPoint(_)) => "ST_MultiPoint",
            (Style::Prefixed, MultiLineString(_)) => "ST_MultiLineString",
            (Style::Prefixed, MultiPolygon(_)) => "ST_MultiPolygon",
            (Style::Prefixed, GeometryCollection(_)) => "ST_GeometryCollection",
        };
        Ok(Some(name))
    }
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    #[tokio::test]
    async fn test_geometry_type() {
        let ctx = SessionContext::new();

        ctx.register_udf(GeometryType::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT GeometryType(ST_GeomFromText('LINESTRING(77.29 29.07,77.42 29.26,77.27 29.31,77.29 29.07)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_string::<i32>().value(0);
        assert_eq!(val, "LINESTRING");
    }

    #[tokio::test]
    async fn test_st_geometry_type() {
        let ctx = SessionContext::new();

        ctx.register_udf(ST_GeometryType::new().into());
        ctx.register_udf(GeomFromText::new().into());

        let df = ctx
            .sql("SELECT ST_GeometryType(ST_GeomFromText('LINESTRING(77.29 29.07,77.42 29.26,77.27 29.31,77.29 29.07)'));")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let col = batch.column(0);
        let val = col.as_string::<i32>().value(0);
        assert_eq!(val, "ST_LineString");
    }
}
