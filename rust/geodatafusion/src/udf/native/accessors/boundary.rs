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
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::editors::collection_homogenize::homogenize;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, to_owned_geometry};
use crate::util::signature::single_geometry;

/// Returns the boundary of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the boundary of a geometry: nothing for points, the endpoints of an open LINESTRING as a MULTIPOINT, and the rings of polygons as a LINESTRING or MULTILINESTRING. For a MULTILINESTRING, an endpoint is on the boundary if an odd number of lines end there (the mod-2 rule). For a MULTIPOLYGON or GEOMETRYCOLLECTION, the boundaries of the members are combined as ST_CollectionHomogenize does. An empty geometry has an empty boundary.",
    syntax_example = "ST_Boundary(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_exteriorring"),
    related_udf(name = "st_makepolygon")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Boundary;

impl Boundary {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Boundary {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Boundary {
    fn name(&self) -> &str {
        "st_boundary"
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
        Ok(boundary_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn boundary_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &BoundaryKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct BoundaryKernel;

impl GeometryKernel for BoundaryKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        Ok(Some(boundary(to_owned_geometry(geom))))
    }
}

/// The boundary of a geometry, by PostGIS's rules as recorded.
fn boundary(geom: Wkt<f64>) -> Wkt<f64> {
    let dim = dimension(geom.dim());
    match geom {
        Wkt::Point(_) => Wkt::Point(Point::new(None, dim)),
        Wkt::MultiPoint(_) => Wkt::MultiPoint(MultiPoint::new(vec![], dim)),
        Wkt::LineString(line) => endpoints(std::iter::once(line), dim),
        Wkt::MultiLineString(lines) => endpoints(lines.into_inner().0.into_iter(), dim),
        Wkt::Polygon(polygon) => {
            let rings = non_empty_rings(polygon);
            match <[_; 1]>::try_from(rings) {
                Ok([ring]) => Wkt::LineString(ring),
                Err(rings) => Wkt::MultiLineString(MultiLineString::new(rings, dim)),
            }
        }
        Wkt::MultiPolygon(polygons) => {
            let rings = polygons
                .into_inner()
                .0
                .into_iter()
                .flat_map(non_empty_rings)
                .map(Wkt::LineString)
                .collect();
            homogenize(Wkt::GeometryCollection(GeometryCollection::new(rings, dim)))
        }
        Wkt::GeometryCollection(collection) => {
            let members = collection
                .into_inner()
                .0
                .into_iter()
                .map(boundary)
                .collect();
            homogenize(Wkt::GeometryCollection(GeometryCollection::new(
                members, dim,
            )))
        }
    }
}

fn non_empty_rings(polygon: Polygon<f64>) -> Vec<LineString<f64>> {
    polygon
        .into_inner()
        .0
        .into_iter()
        .filter(|ring| !ring.coords().is_empty())
        .collect()
}

/// The endpoints on the boundary of some lines, by the mod-2 rule: each endpoint of a non-empty
/// line toggles in or out of the result, which keeps the order PostGIS gives. All ordinates are
/// compared, so a line whose ends differ only in Z isn't closed.
fn endpoints(lines: impl Iterator<Item = LineString<f64>>, dim: Dimension) -> Wkt<f64> {
    let mut boundary: Vec<Coord<f64>> = vec![];
    for line in lines {
        let coords = line.coords();
        let (Some(first), Some(last)) = (coords.first(), coords.last()) else {
            continue;
        };
        for end in [*first, *last] {
            match boundary.iter().position(|coord| *coord == end) {
                Some(index) => {
                    boundary.remove(index);
                }
                None => boundary.push(end),
            }
        }
    }
    let points = boundary
        .into_iter()
        .map(|coord| Point::new(Some(coord), dim))
        .collect();
    Wkt::MultiPoint(MultiPoint::new(points, dim))
}
