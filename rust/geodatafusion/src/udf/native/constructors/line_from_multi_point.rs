use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, MultiPointTrait, PointTrait};
use wkt::Wkt;
use wkt::types::LineString;

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_coord};
use crate::util::signature::single_geometry;

/// Creates a LineString from a MultiPoint geometry.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns a LINESTRING through the points of a MULTIPOINT, in order, keeping Z and M. A MULTIPOINT of one point gives a one-point LINESTRING, as in PostGIS; an empty point in it is an error.",
    syntax_example = "ST_LineFromMultiPoint(aMultiPoint)",
    argument(name = "aMultiPoint", description = "geometry"),
    related_udf(name = "st_makeline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineFromMultiPoint;

impl LineFromMultiPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineFromMultiPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineFromMultiPoint {
    fn name(&self) -> &str {
        "st_linefrommultipoint"
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
        Ok(line_from_multi_point_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_from_multi_point_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(
        geometries.as_ref(),
        &LineFromMultiPointKernel,
        &args.return_field,
    )?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineFromMultiPointKernel;

impl GeometryKernel for LineFromMultiPointKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let GeometryType::MultiPoint(points) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "st_linefrommultipoint: makeline: input must be a multipoint"
            )
            .into());
        };
        let coords = points
            .points()
            .map(|point| {
                point
                    .coord()
                    .map(|coord| owned_coord(&coord))
                    .ok_or_else(|| {
                        exec_datafusion_err!(
                            "st_linefrommultipoint: the multipoint has an empty point"
                        )
                    })
            })
            .collect::<Result<_>>()?;
        Ok(Some(Wkt::LineString(LineString::new(
            coords,
            dimension(geom.dim()),
        ))))
    }
}
