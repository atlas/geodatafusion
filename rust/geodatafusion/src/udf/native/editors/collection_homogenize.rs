use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{
    GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_atoms, to_owned_geometry};
use crate::util::signature::single_geometry;

/// Returns the simplest representation of a geometry collection.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the simplest representation of a collection: its points, linestrings and polygons (looking into nested collections and MULTI* members) grouped by type, a group of one being the geometry itself and a larger one a MULTI* geometry. One group is returned as it is; several make a GEOMETRYCOLLECTION of points, then linestrings, then polygons. An empty geometry gives an empty one of its type, and a geometry that isn't a collection is returned unchanged.",
    syntax_example = "ST_CollectionHomogenize(collection)",
    argument(name = "collection", description = "geometry"),
    related_udf(name = "st_collectionextract")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CollectionHomogenize;

impl CollectionHomogenize {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CollectionHomogenize {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for CollectionHomogenize {
    fn name(&self) -> &str {
        "st_collectionhomogenize"
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
        Ok(collection_homogenize_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn collection_homogenize_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(
        geometries.as_ref(),
        &CollectionHomogenizeKernel,
        &args.return_field,
    )?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct CollectionHomogenizeKernel;

impl GeometryKernel for CollectionHomogenizeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(homogenize(to_owned_geometry(geom))))
    }
}

/// PostGIS's homogenized form of a geometry, as ST_CollectionHomogenize returns it. ST_Boundary
/// homogenizes the boundaries of collection members the same way.
pub(crate) fn homogenize(geom: Wkt<f64>) -> Wkt<f64> {
    let dim = dimension(geom.dim());
    let empty_of_type = match &geom {
        Wkt::MultiPoint(_) => Wkt::MultiPoint(MultiPoint::new(vec![], dim)),
        Wkt::MultiLineString(_) => Wkt::MultiLineString(MultiLineString::new(vec![], dim)),
        Wkt::MultiPolygon(_) => Wkt::MultiPolygon(MultiPolygon::new(vec![], dim)),
        Wkt::GeometryCollection(_) => Wkt::GeometryCollection(GeometryCollection::new(vec![], dim)),
        // Not a collection.
        _ => return geom,
    };
    if is_geometry_topologically_empty(&geom) {
        return empty_of_type;
    }
    // Empty parts stay in their group.
    let mut points: Vec<Point<f64>> = vec![];
    let mut lines: Vec<LineString<f64>> = vec![];
    let mut polygons: Vec<Polygon<f64>> = vec![];
    for atom in owned_atoms(&geom) {
        match atom {
            Wkt::Point(point) => points.push(point),
            Wkt::LineString(line) => lines.push(line),
            Wkt::Polygon(polygon) => polygons.push(polygon),
            _ => {}
        }
    }
    let mut groups = Vec::new();
    if !points.is_empty() {
        groups.push(match <[_; 1]>::try_from(points) {
            Ok([point]) => Wkt::Point(point),
            Err(points) => Wkt::MultiPoint(MultiPoint::new(points, dim)),
        });
    }
    if !lines.is_empty() {
        groups.push(match <[_; 1]>::try_from(lines) {
            Ok([line]) => Wkt::LineString(line),
            Err(lines) => Wkt::MultiLineString(MultiLineString::new(lines, dim)),
        });
    }
    if !polygons.is_empty() {
        groups.push(match <[_; 1]>::try_from(polygons) {
            Ok([polygon]) => Wkt::Polygon(polygon),
            Err(polygons) => Wkt::MultiPolygon(MultiPolygon::new(polygons, dim)),
        });
    }
    match <[_; 1]>::try_from(groups) {
        Ok([group]) => group,
        Err(groups) => Wkt::GeometryCollection(GeometryCollection::new(groups, dim)),
    }
}
