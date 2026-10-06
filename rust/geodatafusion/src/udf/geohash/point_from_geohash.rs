use arrow_array::StringArrayType;
use arrow_array::cast::AsArray;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::WkbArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, WkbType};

use crate::error::GeoDataFusionResult;
use crate::util::field::wkb_return_field;

#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Return a point from a GeoHash string. The point represents the center point of the GeoHash.",
    syntax_example = "ST_PointFromGeoHash(geohash)",
    argument(name = "geohash", description = "text")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointFromGeoHash {
    signature: Signature,
}

impl PointFromGeoHash {
    pub fn new() -> Self {
        Self {
            signature: Signature::uniform(
                1,
                vec![DataType::Utf8, DataType::LargeUtf8, DataType::Utf8View],
                Volatility::Immutable,
            ),
        }
    }
}

impl Default for PointFromGeoHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointFromGeoHash {
    fn name(&self) -> &str {
        "st_pointfromgeohash"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Default::default()))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_from_geohash_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn point_from_geohash_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let array = ColumnarValue::values_to_arrays(&args.args)?
        .into_iter()
        .next()
        .unwrap();

    let GeoArrowType::Wkb(typ) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(
            internal_datafusion_err!("st_pointfromgeohash: unexpected return field").into(),
        );
    };
    let point_arr = match array.data_type() {
        DataType::Utf8 => build_point_arr(typ, &array.as_string::<i32>()),
        DataType::LargeUtf8 => build_point_arr(typ, &array.as_string::<i64>()),
        DataType::Utf8View => build_point_arr(typ, &array.as_string_view()),
        _ => unreachable!(),
    }?;

    Ok(ColumnarValue::Array(point_arr.into_array_ref()))
}

fn build_point_arr<'a>(
    typ: WkbType,
    array: &impl StringArrayType<'a>,
) -> GeoDataFusionResult<WkbArray> {
    let mut builder = WkbBuilder::<i32>::new(typ);
    for s in array.iter() {
        let point = s
            .map(|s| geohash::decode(s).map(|(coord, _, _)| geo::Point(coord)))
            .transpose()?;
        builder.push_geometry(point.as_ref())?;
    }
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use approx::relative_eq;
    use datafusion::prelude::SessionContext;
    use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};
    use geoarrow_array::GeoArrowArrayAccessor;

    use super::*;

    #[tokio::test]
    async fn test_point_from_geohash() {
        let ctx = SessionContext::new();
        ctx.register_udf(PointFromGeoHash::default().into());

        let df = ctx
            .sql("SELECT ST_PointFromGeoHash('9qqj');")
            .await
            .unwrap();

        let schema = df.schema().clone();
        let batches = df.collect().await.unwrap();
        let column = batches[0].column(0);

        let wkb_array = WkbArray::try_from((column.as_ref(), schema.field(0).as_ref())).unwrap();
        let geom = wkb_array.value(0).unwrap();
        let GeometryType::Point(point) = geom.as_type() else {
            panic!("expected a point");
        };

        assert!(relative_eq!(point.coord().unwrap().x(), -115.13671875));
        assert!(relative_eq!(point.coord().unwrap().y(), 36.123046875));
    }
}
