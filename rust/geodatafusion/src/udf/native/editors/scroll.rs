use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Scroll(geometry linestring, geometry point). PostGIS doesn't name the
/// parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Changes the start point of a closed LineString.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the closed LINESTRING rotated to start, and end, at the first of its vertices that is the given POINT. The line must be closed in 2D and contain the point. As in PostGIS, the point is compared ordinate by ordinate with how the line stores its points, a missing Z or M of the point counting as 0; for an XYM line that compares the line's M with the point's Z.",
    syntax_example = "ST_Scroll(linestring, point)",
    argument(name = "linestring", description = "geometry"),
    argument(name = "point", description = "geometry"),
    related_udf(name = "st_normalize")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Scroll;

impl Scroll {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Scroll {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Scroll {
    fn name(&self) -> &str {
        "st_scroll"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
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

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(scroll_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn scroll_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ScrollKernel {
        point: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ScrollKernel {
    point: OwnedColumn,
}

impl GeometryKernel for ScrollKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(point) = self.point.get(row) else {
            return Ok(None);
        };
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_scroll: First argument must be a line").into());
        };
        let Wkt::Point(point) = point else {
            return Err(exec_datafusion_err!("st_scroll: Second argument must be a point").into());
        };
        let Some(point) = point.coord() else {
            return Err(exec_datafusion_err!(
                "st_scroll: Second argument must be a non-empty point"
            )
            .into());
        };
        let dim = dimension(geom.dim());
        let (coords, _) = owned_line_string(line, dim).into_inner();
        let (Some(first), Some(last)) = (coords.first(), coords.last()) else {
            return Err(exec_datafusion_err!(
                "st_scroll: ptarray_scroll_in_place: input POINTARRAY is not closed"
            )
            .into());
        };
        if first.x != last.x || first.y != last.y {
            return Err(exec_datafusion_err!(
                "st_scroll: ptarray_scroll_in_place: input POINTARRAY is not closed"
            )
            .into());
        }
        let Some(start) = coords
            .iter()
            .position(|coord| stored(coord, dim) == as_stored(point, dim))
        else {
            return Err(exec_datafusion_err!(
                "st_scroll: ptarray_scroll_in_place: input POINTARRAY does not contain the given point"
            )
            .into());
        };
        if start == 0 {
            return Ok(Some(Wkt::LineString(LineString::new(coords, dim))));
        }
        // PostGIS drops the first point (the one the line was closed on) and closes the line on
        // the new start.
        let mut scrolled: Vec<Coord<f64>> = coords[start..].to_vec();
        scrolled.extend_from_slice(&coords[1..start]);
        scrolled.push(coords[start]);
        Ok(Some(Wkt::LineString(LineString::new(scrolled, dim))))
    }
}

/// The ordinates a line of dimension `dim` stores for `coord`, in order.
fn stored(coord: &Coord<f64>, dim: Dimension) -> Vec<f64> {
    let mut ordinates = vec![coord.x, coord.y];
    match dim {
        Dimension::XY => {}
        Dimension::XYZ => ordinates.push(coord.z.unwrap_or(0.0)),
        Dimension::XYM => ordinates.push(coord.m.unwrap_or(0.0)),
        Dimension::XYZM => {
            ordinates.push(coord.z.unwrap_or(0.0));
            ordinates.push(coord.m.unwrap_or(0.0));
        }
    }
    ordinates
}

/// The point's x, y, z and m (0 when missing), cut to as many ordinates as the line stores.
fn as_stored(point: &Coord<f64>, dim: Dimension) -> Vec<f64> {
    let all = [
        point.x,
        point.y,
        point.z.unwrap_or(0.0),
        point.m.unwrap_or(0.0),
    ];
    let count = match dim {
        Dimension::XY => 2,
        Dimension::XYZ | Dimension::XYM => 3,
        Dimension::XYZM => 4,
    };
    all[..count].to_vec()
}
