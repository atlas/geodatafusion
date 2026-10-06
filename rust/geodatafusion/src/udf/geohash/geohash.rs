use std::sync::Arc;

use arrow_array::StringViewArray;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::to_geo::ToGeoCoord;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Computes a GeoHash representation of a geometry. A GeoHash encodes a geographic Point into a text form that is sortable and searchable based on prefixing. A shorter GeoHash is a less precise representation of a point. It can be thought of as a box that contains the point.",
    syntax_example = "ST_GeoHash(point)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeoHash;

impl GeoHash {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for GeoHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeoHash {
    fn name(&self) -> &str {
        "st_geohash"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8View)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geohash_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geohash_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: StringViewArray = map_geometry(geometries.as_ref(), &GeoHashKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// The GeoHash of a point.
struct GeoHashKernel;

impl GeometryKernel for GeoHashKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // TODO: PostGIS hashes the bounding box of any geometry, to up to 20 characters.
        let GeometryType::Point(point) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_geohash: only points are supported").into());
        };
        // An EMPTY point has no coordinate, or NaN coordinates in WKB. PostGIS returns NULL.
        let Some(coord) = point
            .coord()
            .filter(|c| !(c.x().is_nan() && c.y().is_nan()))
        else {
            return Ok(None);
        };
        // 12 characters is the most the geohash crate supports.
        Ok(Some(geohash::encode(coord.to_coord(), 12)?))
    }
}

#[cfg(test)]
mod test {
    use arrow_array::Array;
    use arrow_array::cast::AsArray;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::constructors::Point;
    use crate::udf::native::io::GeomFromEWKT;

    #[tokio::test]
    async fn test_geohash() {
        let ctx = SessionContext::new();
        ctx.register_udf(GeoHash.into());
        ctx.register_udf(Point::default().into());

        let df = ctx
            .sql("SELECT ST_GeoHash( ST_Point(-126,48) );")
            .await
            .unwrap();

        let batches = df.collect().await.unwrap();
        let column = batches[0].column(0);
        let string_arr = column.as_string_view();

        assert_eq!(string_arr.value(0), "c0w3hf1s70w3");
    }

    #[tokio::test]
    async fn test_geohash_wkb_and_empty() {
        let ctx = SessionContext::new();
        ctx.register_udf(GeoHash.into());
        ctx.register_udf(GeomFromEWKT.into());

        let df = ctx
            .sql("SELECT ST_GeoHash(ST_GeomFromEWKT(g)) FROM (VALUES ('POINT ZM (-126 48 3 4)'), ('POINT EMPTY')) AS t(g);")
            .await
            .unwrap();

        let batches = df.collect().await.unwrap();
        let string_arr = batches[0].column(0).as_string_view();

        assert_eq!(string_arr.value(0), "c0w3hf1s70w3");
        assert!(string_arr.is_null(1));
    }
}
