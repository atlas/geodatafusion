use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString, Point, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::single_geometry;

/// Returns a geometry representing the bounding box of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the 2D bounding box of a geometry as a geometry: a POLYGON with the corners (minx miny, minx maxy, maxx maxy, maxx miny), a LINESTRING if the box is a vertical or horizontal line, or a POINT if it is a point. An empty geometry is returned unchanged. The result drops Z and M.",
    syntax_example = "ST_Envelope(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "box2d"),
    related_udf(name = "st_boundingdiagonal")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Envelope;

impl Envelope {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Envelope {
    fn name(&self) -> &str {
        "st_envelope"
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
        Ok(envelope_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn envelope_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &EnvelopeKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct EnvelopeKernel;

impl GeometryKernel for EnvelopeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        if rect.is_empty() {
            return Ok(Some(to_owned_geometry(geom)));
        }
        let corner = |x, y| Coord {
            x,
            y,
            z: None,
            m: None,
        };
        let (min, max) = (
            corner(rect.minx(), rect.miny()),
            corner(rect.maxx(), rect.maxy()),
        );
        let degenerate_x = rect.minx() == rect.maxx();
        let degenerate_y = rect.miny() == rect.maxy();
        Ok(Some(match (degenerate_x, degenerate_y) {
            (true, true) => Wkt::Point(Point::new(Some(min), Dimension::XY)),
            (true, false) | (false, true) => {
                Wkt::LineString(LineString::new(vec![min, max], Dimension::XY))
            }
            (false, false) => {
                let ring = vec![
                    min,
                    corner(rect.minx(), rect.maxy()),
                    max,
                    corner(rect.maxx(), rect.miny()),
                    min,
                ];
                Wkt::Polygon(Polygon::new(
                    vec![LineString::new(ring, Dimension::XY)],
                    Dimension::XY,
                ))
            }
        }))
    }
}
