use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::Wkt;
use wkt::types::{MultiLineString, MultiPoint, MultiPolygon};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{
    dimension, owned_line_string, owned_point, owned_polygon, to_owned_geometry,
};
use crate::util::signature::single_geometry;

/// Return the geometry as a MULTI* geometry.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns a POINT, LINESTRING or POLYGON as a MULTI* geometry with it as the only member, or an empty MULTI* geometry if it is empty. Collections are returned unchanged.",
    syntax_example = "ST_Multi(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_forcecollection")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Multi;

impl Multi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Multi {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Multi {
    fn name(&self) -> &str {
        "st_multi"
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
        Ok(multi_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn multi_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &MultiKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct MultiKernel;

impl GeometryKernel for MultiKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let wkt_dim = dimension(geom.dim());
        let empty = is_geometry_topologically_empty(geom);
        Ok(Some(match geom.as_type() {
            GeometryType::Point(point) => Wkt::MultiPoint(MultiPoint::new(
                if empty {
                    vec![]
                } else {
                    vec![owned_point(point, wkt_dim)]
                },
                wkt_dim,
            )),
            GeometryType::LineString(line) => Wkt::MultiLineString(MultiLineString::new(
                if empty {
                    vec![]
                } else {
                    vec![owned_line_string(line, wkt_dim)]
                },
                wkt_dim,
            )),
            GeometryType::Polygon(polygon) => Wkt::MultiPolygon(MultiPolygon::new(
                if empty {
                    vec![]
                } else {
                    vec![owned_polygon(polygon, wkt_dim)]
                },
                wkt_dim,
            )),
            _ => to_owned_geometry(geom),
        }))
    }
}
