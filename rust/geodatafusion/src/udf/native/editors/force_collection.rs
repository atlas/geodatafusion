use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryTrait, GeometryType, MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait,
};
use wkt::Wkt;
use wkt::types::GeometryCollection;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{
    dimension, owned_line_string, owned_point, owned_polygon, to_owned_geometry,
};
use crate::util::signature::single_geometry;

/// Convert the geometry into a GEOMETRYCOLLECTION.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry as a GEOMETRYCOLLECTION: a single geometry becomes its only member, even if empty, and the members of a MULTI* geometry become members of their own. A GEOMETRYCOLLECTION is returned unchanged.",
    syntax_example = "ST_ForceCollection(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_multi")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ForceCollection;

impl ForceCollection {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ForceCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ForceCollection {
    fn name(&self) -> &str {
        "st_forcecollection"
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
        Ok(force_collection_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn force_collection_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(
        geometries.as_ref(),
        &ForceCollectionKernel,
        &args.return_field,
    )?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ForceCollectionKernel;

impl GeometryKernel for ForceCollectionKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let wkt_dim = dimension(geom.dim());
        let members = match geom.as_type() {
            GeometryType::MultiPoint(points) => points
                .points()
                .map(|point| Wkt::Point(owned_point(&point, wkt_dim)))
                .collect(),
            GeometryType::MultiLineString(lines) => lines
                .line_strings()
                .map(|line| Wkt::LineString(owned_line_string(&line, wkt_dim)))
                .collect(),
            GeometryType::MultiPolygon(polygons) => polygons
                .polygons()
                .map(|polygon| Wkt::Polygon(owned_polygon(&polygon, wkt_dim)))
                .collect(),
            GeometryType::GeometryCollection(_) => return Ok(Some(to_owned_geometry(geom))),
            _ => vec![to_owned_geometry(geom)],
        };
        Ok(Some(Wkt::GeometryCollection(GeometryCollection::new(
            members, wkt_dim,
        ))))
    }
}
