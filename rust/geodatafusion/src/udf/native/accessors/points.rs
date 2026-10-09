use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, MultiLineStringTrait,
    MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};
use wkt::types::{Coord, MultiPoint, Point};

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_coord, to_owned_geometry};
use crate::util::signature::single_geometry;

/// Returns a MultiPoint containing the coordinates of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns a MULTIPOINT of every coordinate of a geometry, in order, keeping repeated points (including the closing point of rings) and Z and M. An empty geometry gives MULTIPOINT EMPTY.",
    syntax_example = "ST_Points(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_npoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Points;

impl Points {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Points {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Points {
    fn name(&self) -> &str {
        "st_points"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
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

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(points_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn points_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &PointsKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PointsKernel;

impl GeometryKernel for PointsKernel {
    type Output = MultiPoint<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<MultiPoint<f64>>> {
        let dim = dimension(geom.dim());
        let mut coords = Vec::new();
        collect_coords(geom, &mut coords);
        let points = coords
            .into_iter()
            .map(|coord| Point::new(Some(coord), dim))
            .collect();
        Ok(Some(MultiPoint::new(points, dim)))
    }
}

/// Appends every coordinate of `geom` to `coords`. Empty points have none.
fn collect_coords(geom: &impl GeometryTrait<T = f64>, coords: &mut Vec<Coord<f64>>) {
    match geom.as_type() {
        GeometryType::Point(point) => coords.extend(point.coord().map(|c| owned_coord(&c))),
        GeometryType::LineString(line) => line_coords(line, coords),
        GeometryType::Polygon(polygon) => polygon_coords(polygon, coords),
        GeometryType::MultiPoint(points) => {
            for point in points.points() {
                coords.extend(point.coord().map(|c| owned_coord(&c)));
            }
        }
        GeometryType::MultiLineString(lines) => {
            for line in lines.line_strings() {
                line_coords(&line, coords);
            }
        }
        GeometryType::MultiPolygon(polygons) => {
            for polygon in polygons.polygons() {
                polygon_coords(&polygon, coords);
            }
        }
        GeometryType::GeometryCollection(collection) => {
            for member in collection.geometries() {
                collect_coords(&member, coords);
            }
        }
        // The coordinates of the polygon or linestring PostGIS would see.
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {
            collect_coords(&to_owned_geometry(geom), coords)
        }
    }
}

fn line_coords(line: &impl LineStringTrait<T = f64>, coords: &mut Vec<Coord<f64>>) {
    coords.extend(line.coords().map(|c| owned_coord(&c)));
}

fn polygon_coords(polygon: &impl PolygonTrait<T = f64>, coords: &mut Vec<Coord<f64>>) {
    for ring in polygon.exterior().into_iter().chain(polygon.interiors()) {
        line_coords(&ring, coords);
    }
}
