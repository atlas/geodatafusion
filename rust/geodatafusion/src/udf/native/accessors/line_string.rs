//! Accessors from LineString geometries

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};
use wkt::types::{Coord, Point};

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::ordinates::{m, z};
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the first point of a geometry as a POINT: the first point of a LINESTRING, or of the first ring or member of other geometry types. Returns NULL if the geometry, or its first ring or member, is empty.",
    syntax_example = "ST_StartPoint(geom)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct StartPoint;

impl StartPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StartPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for StartPoint {
    fn name(&self) -> &str {
        "st_startpoint"
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
        Ok(point_impl(args, Mode::Start)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the last point of a LINESTRING geometry as a POINT. Returns NULL if the input is not a LINESTRING.",
    syntax_example = "ST_EndPoint(line_string)",
    argument(name = "g1", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct EndPoint;

impl EndPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EndPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for EndPoint {
    fn name(&self) -> &str {
        "st_endpoint"
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
        Ok(point_impl(args, Mode::End)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Start,
    End,
}

fn point_impl(args: ScalarFunctionArgs, mode: Mode) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &mode, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

impl GeometryKernel for Mode {
    type Output = Point<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Point<f64>>> {
        Ok(match self {
            Mode::Start => start_point(geom),
            // PostGIS only has an end point for linear geometries.
            Mode::End => match geom.as_type() {
                GeometryType::LineString(line) => line
                    .coord(line.num_coords().wrapping_sub(1))
                    .map(coord_to_point),
                _ => None,
            },
        })
    }
}

/// The first point of a geometry, as in PostGIS: the first point of its first ring or member.
/// `None` if that is empty.
fn start_point(geom: &impl GeometryTrait<T = f64>) -> Option<Point<f64>> {
    match geom.as_type() {
        GeometryType::Point(point) => point.coord().map(coord_to_point),
        GeometryType::LineString(line) => line.coord(0).map(coord_to_point),
        GeometryType::Polygon(polygon) => polygon.exterior()?.coord(0).map(coord_to_point),
        GeometryType::MultiPoint(points) => start_point(&points.point(0)?),
        GeometryType::MultiLineString(lines) => start_point(&lines.line_string(0)?),
        GeometryType::MultiPolygon(polygons) => start_point(&polygons.polygon(0)?),
        GeometryType::GeometryCollection(collection) => start_point(&collection.geometry(0)?),
        GeometryType::Rect(rect) => Some(coord_to_point(rect.min())),
        GeometryType::Triangle(triangle) => Some(coord_to_point(triangle.first())),
        GeometryType::Line(line) => Some(coord_to_point(line.start())),
    }
}

/// A coordinate as a point, keeping its Z and M.
fn coord_to_point(coord: impl CoordTrait<T = f64>) -> Point<f64> {
    Point::from_coord(Coord {
        x: coord.x(),
        y: coord.y(),
        z: z(&coord),
        m: m(&coord),
    })
}
