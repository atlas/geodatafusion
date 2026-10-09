use std::sync::Arc;

use arrow_array::StringArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::{
    line_string_to_owned, point_to_owned, polygon_to_owned, to_owned_geometry,
};
use crate::util::signature::single_geometry;
use crate::util::srid::crs_to_srid;

/// Returns a text summary of the contents of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns a text summary of a geometry: its type, flags and size, with one line per member of a collection and per polygon ring. The flags are Z and M for the dimension, S when the geometry has an SRID, and B where PostGIS would store a bounding box with the geometry: at the top level, unless it is empty, a point, a two-point line, or a MULTIPOINT or MULTILINESTRING of one such member.",
    syntax_example = "ST_Summary(g)",
    argument(name = "g", description = "geometry"),
    related_udf(name = "st_geometrytype"),
    related_udf(name = "st_npoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Summary;

impl Summary {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Summary {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Summary {
    fn name(&self) -> &str {
        "st_summary"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(summary_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn summary_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = SummaryKernel {
        has_srid: crs_to_srid(input_metadata(&args.arg_fields[0]).crs()) != Some(0),
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct SummaryKernel {
    /// Whether the column has an SRID; one per column.
    has_srid: bool,
}

impl GeometryKernel for SummaryKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        let mut out = String::new();
        let bbox = needs_bbox(geom);
        match geom.as_type() {
            // As the polygon or linestring PostGIS would see.
            GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {
                self.write(&mut out, &to_owned_geometry(geom), 0, bbox)
            }
            _ => self.write(&mut out, geom, 0, bbox),
        }
        Ok(Some(out))
    }
}

impl SummaryKernel {
    fn write(
        &self,
        out: &mut String,
        geom: &impl GeometryTrait<T = f64>,
        offset: usize,
        bbox: bool,
    ) {
        let indent = " ".repeat(offset);
        let type_name = match geom.as_type() {
            GeometryType::Point(_) => "Point",
            GeometryType::LineString(_) | GeometryType::Line(_) => "LineString",
            GeometryType::Polygon(_) | GeometryType::Rect(_) | GeometryType::Triangle(_) => {
                "Polygon"
            }
            GeometryType::MultiPoint(_) => "MultiPoint",
            GeometryType::MultiLineString(_) => "MultiLineString",
            GeometryType::MultiPolygon(_) => "MultiPolygon",
            GeometryType::GeometryCollection(_) => "GeometryCollection",
        };
        let mut flags = String::new();
        if matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm) {
            flags.push('Z');
        }
        if matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
            flags.push('M');
        }
        if bbox {
            flags.push('B');
        }
        if self.has_srid {
            flags.push('S');
        }
        out.push_str(&format!("{indent}{type_name}[{flags}]"));
        let members = |out: &mut String, count: usize| {
            out.push_str(&format!(" with {count} {}", plural(count, "element")));
            if count > 0 {
                out.push(':');
            }
        };
        match geom.as_type() {
            GeometryType::LineString(line) => {
                let count = line.num_coords();
                out.push_str(&format!(" with {count} points"));
            }
            GeometryType::Polygon(polygon) => {
                let rings: Vec<_> = polygon
                    .exterior()
                    .filter(|ring| ring.num_coords() > 0)
                    .into_iter()
                    .chain(polygon.interiors())
                    .collect();
                out.push_str(&format!(
                    " with {} {}",
                    rings.len(),
                    plural(rings.len(), "ring")
                ));
                if !rings.is_empty() {
                    out.push(':');
                }
                // PostGIS indents rings by three spaces at any depth.
                for (index, ring) in rings.iter().enumerate() {
                    out.push_str(&format!(
                        "\n   ring {index} has {} points",
                        ring.num_coords()
                    ));
                }
            }
            GeometryType::MultiPoint(points) => {
                members(out, points.num_points());
                for point in points.points() {
                    out.push('\n');
                    self.write_point(out, &point, offset + 2, geom.dim());
                }
            }
            GeometryType::MultiLineString(lines) => {
                members(out, lines.num_line_strings());
                for line in lines.line_strings() {
                    out.push('\n');
                    let line = line_string_to_owned(&line, geom.dim());
                    self.write(out, &line, offset + 2, false);
                }
            }
            GeometryType::MultiPolygon(polygons) => {
                members(out, polygons.num_polygons());
                for polygon in polygons.polygons() {
                    out.push('\n');
                    let polygon = polygon_to_owned(&polygon, geom.dim());
                    self.write(out, &polygon, offset + 2, false);
                }
            }
            GeometryType::GeometryCollection(collection) => {
                members(out, collection.num_geometries());
                for member in collection.geometries() {
                    out.push('\n');
                    self.write(out, &member, offset + 2, false);
                }
            }
            _ => {}
        }
    }

    fn write_point(
        &self,
        out: &mut String,
        point: &impl PointTrait<T = f64>,
        offset: usize,
        dim: Dimensions,
    ) {
        let point = point_to_owned(point, dim);
        self.write(out, &point, offset, false);
    }
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Whether PostGIS stores a bounding box with the geometry, as recorded: never when it is
/// empty, for a point or a two-point line, nor for a MULTIPOINT or MULTILINESTRING of one such
/// member; always otherwise.
fn needs_bbox(geom: &impl GeometryTrait<T = f64>) -> bool {
    if is_geometry_topologically_empty(geom) {
        return false;
    }
    match geom.as_type() {
        GeometryType::Point(_) => false,
        GeometryType::LineString(line) => line.num_coords() > 2,
        GeometryType::Line(_) => false,
        GeometryType::MultiPoint(points) => points.num_points() > 1,
        GeometryType::MultiLineString(lines) => {
            lines.num_line_strings() > 1 || lines.line_strings().any(|line| line.num_coords() > 2)
        }
        _ => true,
    }
}
