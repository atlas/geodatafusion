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
    MultiPolygonTrait, PolygonTrait,
};
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::util::geometry_dump::{
    GeometryDump, geometry_dump_array, geometry_dump_field,
};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::{dimension, owned_coord};
use crate::util::signature::single_geometry;

/// Returns a set of geometry_dump rows for the segments in a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "A set-returning function that extracts the segments of the lines and polygon rings of a geometry as two-point lines, with Z and M, each with its path: the 1-based indexes of the part (through MULTI geometries and collections), the ring for polygons, and the segment. Points have no segments. Returns a list of (path, geom) records per row: unnest it for rows, or call st_dumpsegments(geom) in FROM with a constant argument.",
    syntax_example = "ST_DumpSegments(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_dump"),
    related_udf(name = "st_dumppoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DumpSegments;

impl DumpSegments {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DumpSegments {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DumpSegments {
    fn name(&self) -> &str {
        "st_dumpsegments"
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
        Ok(dump_segments_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dump_segments_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let rows: Vec<Option<GeometryDump>> = map_geometry(geometries.as_ref(), &DumpSegmentsKernel)?;
    let result = geometry_dump_array(rows, &args.return_field)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DumpSegmentsKernel;

impl GeometryKernel for DumpSegmentsKernel {
    type Output = GeometryDump;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<GeometryDump>> {
        let mut dump = vec![];
        dump_segments(geom, dimension(geom.dim()), &mut vec![], &mut dump);
        Ok(Some(dump))
    }
}

/// Adds the segments of the lines and rings of `geom`, under `path`.
fn dump_segments(
    geom: &impl GeometryTrait<T = f64>,
    dim: Dimension,
    path: &mut Vec<i32>,
    dump: &mut GeometryDump,
) {
    match geom.as_type() {
        GeometryType::LineString(line) => {
            push_segments(line.coords().map(|c| owned_coord(&c)), dim, path, dump);
        }
        GeometryType::Polygon(polygon) => {
            let rings = polygon.exterior().into_iter().chain(polygon.interiors());
            for (ring_index, ring) in (1..).zip(rings) {
                path.push(ring_index);
                push_segments(ring.coords().map(|c| owned_coord(&c)), dim, path, dump);
                path.pop();
            }
        }
        GeometryType::MultiLineString(lines) => dump_parts(lines.line_strings(), dim, path, dump),
        GeometryType::MultiPolygon(polygons) => dump_parts(polygons.polygons(), dim, path, dump),
        GeometryType::GeometryCollection(collection) => {
            dump_parts(collection.geometries(), dim, path, dump)
        }
        // Points have no segments.
        _ => {}
    }
}

/// Adds the segments between consecutive coordinates, numbered from 1 under `path`.
fn push_segments(
    coords: impl Iterator<Item = Coord<f64>>,
    dim: Dimension,
    path: &mut Vec<i32>,
    dump: &mut GeometryDump,
) {
    let coords: Vec<Coord<f64>> = coords.collect();
    for (index, segment) in (1..).zip(coords.windows(2)) {
        path.push(index);
        let segment = LineString::new(segment.to_vec(), dim);
        dump.push((path.clone(), Wkt::LineString(segment)));
        path.pop();
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
        dump_segments(&part, dim, path, dump);
        path.pop();
    }
}
