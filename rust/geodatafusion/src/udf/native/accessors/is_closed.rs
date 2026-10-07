use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, LineStringTrait, LineTrait,
    MultiLineStringTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::z;
use crate::util::signature::single_geometry;

#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns TRUE if the LINESTRING's start and end points are coincident, comparing Z but not M. A MULTILINESTRING or GEOMETRYCOLLECTION is closed if all of its members are. Points and polygons are closed, and an empty geometry is not.",
    syntax_example = "ST_IsClosed(g)",
    argument(name = "g", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsClosed;

impl IsClosed {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for IsClosed {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsClosed {
    fn name(&self) -> &str {
        "st_isclosed"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_closed_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_closed_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsClosedKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsClosedKernel;

impl GeometryKernel for IsClosedKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        Ok(Some(is_closed(geom)))
    }
}

/// Whether a geometry is closed, as in PostGIS: a linestring whose first and last points are
/// equal in X, Y and Z (M doesn't count), a multilinestring or collection whose members are all
/// closed, or any other non-empty geometry. EMPTY is never closed.
fn is_closed(geom: &impl GeometryTrait<T = f64>) -> bool {
    use geo_traits::GeometryType::*;

    if is_geometry_topologically_empty(geom) {
        return false;
    }
    match geom.as_type() {
        LineString(line) => is_line_string_closed(line),
        Line(line) => same_position(&line.start(), &line.end()),
        MultiLineString(lines) => lines.line_strings().all(|line| is_closed(&line)),
        GeometryCollection(collection) => collection.geometries().all(|member| is_closed(&member)),
        Point(_) | MultiPoint(_) | Polygon(_) | MultiPolygon(_) | Rect(_) | Triangle(_) => true,
    }
}

fn is_line_string_closed(line: &impl LineStringTrait<T = f64>) -> bool {
    match (line.coord(0), line.coord(line.num_coords().wrapping_sub(1))) {
        (Some(first), Some(last)) => same_position(&first, &last),
        _ => false,
    }
}

/// Whether two coordinates are equal in X, Y and Z.
fn same_position(a: &impl CoordTrait<T = f64>, b: &impl CoordTrait<T = f64>) -> bool {
    a.x() == b.x() && a.y() == b.y() && z(a) == z(b)
}
