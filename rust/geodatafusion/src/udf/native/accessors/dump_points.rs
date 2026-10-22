use std::sync::Arc;

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
use wkt::Wkt;
use wkt::types::{Coord, Dimension, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::util::geometry_dump::{
    GeometryDump, geometry_dump_array, geometry_dump_field,
};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::{dimension, owned_coord};
use crate::util::signature::single_geometry;

/// Returns a set of geometry_dump rows for the coordinates in a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "A set-returning function that extracts the points of a geometry, with Z and M, each with its path: the 1-based indexes of the part (through MULTI geometries and collections), the ring for polygons, and the point. A point's path ends in 1. Empty parts have no points, but count in the indexes. Returns a list of (path, geom) records per row: unnest it for rows, or call st_dumppoints(geom) in FROM with a constant argument.",
    syntax_example = "ST_DumpPoints(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_dump"),
    related_udf(name = "st_dumpsegments")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DumpPoints;

impl DumpPoints {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DumpPoints {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DumpPoints {
    fn name(&self) -> &str {
        "st_dumppoints"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_dump_field(self.name(), &args.arg_fields[0]))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dump_points_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dump_points_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let rows: Vec<Option<GeometryDump>> = map_geometry(geometries.as_ref(), &DumpPointsKernel)?;
    let result = geometry_dump_array(rows, &args.return_field)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DumpPointsKernel;

impl GeometryKernel for DumpPointsKernel {
    type Output = GeometryDump;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<GeometryDump>> {
        let mut dump = vec![];
        dump_points(geom, dimension(geom.dim()), &mut vec![], &mut dump);
        Ok(Some(dump))
    }
}

/// Adds the points of `geom`, under `path`. A point's own path ends in 1, like a line's first
/// point's.
fn dump_points(
    geom: &impl GeometryTrait<T = f64>,
    dim: Dimension,
    path: &mut Vec<i32>,
    dump: &mut GeometryDump,
) {
    let mut push = |path: &[i32], coord: Coord<f64>| {
        dump.push((path.to_vec(), Wkt::Point(Point::new(Some(coord), dim))));
    };
    match geom.as_type() {
        GeometryType::Point(point) => {
            if let Some(coord) = point.coord() {
                path.push(1);
                push(path, owned_coord(&coord));
                path.pop();
            }
        }
        GeometryType::LineString(line) => {
            for (index, coord) in (1..).zip(line.coords()) {
                path.push(index);
                push(path, owned_coord(&coord));
                path.pop();
            }
        }
        GeometryType::Polygon(polygon) => {
            let rings = polygon.exterior().into_iter().chain(polygon.interiors());
            for (ring_index, ring) in (1..).zip(rings) {
                path.push(ring_index);
                for (index, coord) in (1..).zip(ring.coords()) {
                    path.push(index);
                    push(path, owned_coord(&coord));
                    path.pop();
                }
                path.pop();
            }
        }
        GeometryType::MultiPoint(points) => dump_parts(points.points(), dim, path, dump),
        GeometryType::MultiLineString(lines) => dump_parts(lines.line_strings(), dim, path, dump),
        GeometryType::MultiPolygon(polygons) => dump_parts(polygons.polygons(), dim, path, dump),
        GeometryType::GeometryCollection(collection) => {
            dump_parts(collection.geometries(), dim, path, dump)
        }
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {}
    }
}

fn dump_parts(
    parts: impl Iterator<Item = impl GeometryTrait<T = f64>>,
    dim: Dimension,
    path: &mut Vec<i32>,
    dump: &mut GeometryDump,
) {
    for (index, part) in (1..).zip(parts) {
        path.push(index);
        dump_points(&part, dim, path, dump);
        path.pop();
    }
}
