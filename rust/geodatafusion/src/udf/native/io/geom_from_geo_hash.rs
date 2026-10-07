//! Constructors from GeoHash strings: ST_PointFromGeoHash, ST_GeomFromGeoHash and
//! ST_Box2DFromGeoHash.

use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::{RectBuilder, WkbBuilder};
use geoarrow_schema::{BoxType, Dimension, GeoArrowType};
use wkt::types::{Coord, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::io::util::geohash::{Bounds, cell_geometry, center, decode};
use crate::util::args::optional_int_arg;
use crate::util::field::wkb_return_field;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_PointFromGeoHash(text geohash, integer precision = NULL), and the same for
/// ST_GeomFromGeoHash and ST_Box2DFromGeoHash. PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text], &[Arg::Text, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Returns the centre of a GeoHash cell as a point.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns a point from a GeoHash string: the centre of the GeoHash cell. If precision is given, only that many characters of the GeoHash are used; if it's NULL or negative, the whole GeoHash is. The point has no SRID.",
    syntax_example = "ST_PointFromGeoHash(geohash, precision)",
    argument(name = "geohash", description = "text"),
    argument(name = "precision", description = "integer"),
    related_udf(name = "st_geohash"),
    related_udf(name = "st_geomfromgeohash"),
    related_udf(name = "st_box2dfromgeohash")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointFromGeoHash;

impl PointFromGeoHash {
    pub fn new() -> Self {
        Self
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
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Default::default()))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_geo_hash_impl(self.name(), args, Output::Point)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a GeoHash cell as a polygon.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the GeoHash cell of a GeoHash string as a polygon (a point or a line where the cell is too small for double precision). If precision is given, only that many characters of the GeoHash are used; if it's NULL or negative, the whole GeoHash is. The geometry has no SRID.",
    syntax_example = "ST_GeomFromGeoHash(geohash, precision)",
    argument(name = "geohash", description = "text"),
    argument(name = "precision", description = "integer"),
    related_udf(name = "st_geohash"),
    related_udf(name = "st_pointfromgeohash"),
    related_udf(name = "st_box2dfromgeohash")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromGeoHash;

impl GeomFromGeoHash {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomFromGeoHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromGeoHash {
    fn name(&self) -> &str {
        "st_geomfromgeohash"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Default::default()))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_geo_hash_impl(self.name(), args, Output::Cell)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a GeoHash cell as a box2d.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the GeoHash cell of a GeoHash string as a box2d. If precision is given, only that many characters of the GeoHash are used (0 gives the whole world); if it's NULL or negative, the whole GeoHash is.",
    syntax_example = "ST_Box2dFromGeoHash(geohash, precision)",
    argument(name = "geohash", description = "text"),
    argument(name = "precision", description = "integer"),
    related_udf(name = "st_geohash"),
    related_udf(name = "st_pointfromgeohash"),
    related_udf(name = "st_geomfromgeohash")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Box2DFromGeoHash;

impl Box2DFromGeoHash {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Box2DFromGeoHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Box2DFromGeoHash {
    fn name(&self) -> &str {
        "st_box2dfromgeohash"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        let output_type = BoxType::new(Dimension::XY, Default::default());
        Ok(Arc::new(output_type.to_field(self.name(), true)))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_geo_hash_impl(self.name(), args, Output::Box)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// What a decoded cell becomes.
#[derive(Debug, Clone, Copy)]
enum Output {
    Point,
    Cell,
    Box,
}

fn geom_from_geo_hash_impl(
    name: &str,
    args: ScalarFunctionArgs,
    output: Output,
) -> GeoDataFusionResult<ColumnarValue> {
    let cells = decode_rows(name, &args)?;
    let array: ArrayRef = match (output, GeoArrowType::from_arrow_field(&args.return_field)?) {
        (Output::Point | Output::Cell, GeoArrowType::Wkb(output_type)) => {
            let mut builder = WkbBuilder::<i32>::new(output_type);
            for cell in &cells {
                match (output, cell) {
                    (_, None) => builder.push_geometry(None::<&Point>)?,
                    (Output::Point, Some(cell)) => {
                        let (x, y) = center(cell);
                        let point = Point::from_coord(Coord {
                            x,
                            y,
                            z: None,
                            m: None,
                        });
                        builder.push_geometry(Some(&point))?
                    }
                    (_, Some(cell)) => builder.push_geometry(Some(&cell_geometry(cell)))?,
                }
            }
            builder.finish().into_array_ref()
        }
        (Output::Box, GeoArrowType::Rect(output_type)) => {
            let mut builder = RectBuilder::with_capacity(output_type, cells.len());
            for cell in &cells {
                let rect = cell.map(|[xmin, ymin, xmax, ymax]| {
                    BoundingRect::from_raw_bounds(
                        [xmin, ymin, f64::INFINITY, xmax, ymax, f64::NEG_INFINITY],
                        false,
                    )
                });
                builder.push_rect(rect.as_ref());
            }
            builder.finish().into_array_ref()
        }
        _ => return Err(internal_datafusion_err!("{name}: unexpected return field").into()),
    };
    Ok(ColumnarValue::Array(array))
}

/// Decodes the GeoHash of every row; NULL for a NULL GeoHash. A NULL precision reads the whole
/// GeoHash, as in PostGIS.
fn decode_rows(name: &str, args: &ScalarFunctionArgs) -> GeoDataFusionResult<Vec<Option<Bounds>>> {
    let hashes = args.args[0]
        .cast_to(&DataType::Utf8, None)?
        .to_array(args.number_rows)?;
    // Without a precision, the whole GeoHash.
    let precision = optional_int_arg(args, 1, -1)?;
    hashes
        .as_string::<i32>()
        .iter()
        .enumerate()
        .map(|(row, hash)| {
            let Some(hash) = hash else {
                return Ok(None);
            };
            let precision = (!precision.is_null(row)).then(|| precision.value(row));
            let cell = decode(hash, precision).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
            Ok(Some(cell))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use approx::relative_eq;
    use datafusion::prelude::SessionContext;
    use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};
    use geoarrow_array::GeoArrowArrayAccessor;
    use geoarrow_array::array::WkbArray;

    use super::*;

    #[tokio::test]
    async fn test_point_from_geohash() {
        let ctx = SessionContext::new();
        ctx.register_udf(PointFromGeoHash.into());

        let df = ctx.sql("SELECT ST_PointFromGeoHash('9qqj')").await.unwrap();
        let schema = df.schema().clone();
        let batches = df.collect().await.unwrap();
        let wkb =
            WkbArray::try_from((batches[0].column(0).as_ref(), schema.field(0).as_ref())).unwrap();
        let geom = wkb.value(0).unwrap();
        let GeometryType::Point(point) = geom.as_type() else {
            panic!("expected a point");
        };

        assert!(relative_eq!(point.coord().unwrap().x(), -115.13671875));
        assert!(relative_eq!(point.coord().unwrap().y(), 36.123046875));
    }
}
