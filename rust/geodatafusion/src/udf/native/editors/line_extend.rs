use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
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
use wkt::types::{Coord, LineString};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineExtend(geometry line, float distance_forward, float distance_backward = 0.0).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["line", "distance_forward", "distance_backward"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a line extended forwards and backwards by specified distances.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the LINESTRING with a point added beyond its end, distance_forward along the direction of its last segment, and one before its start, distance_backward along its first segment (default 0, no point). Repeated end points are skipped to find the direction, distances are 2D, and Z and M are extrapolated. A line without two distinct points is returned unchanged, an empty one gives NULL, and negative distances are an error.",
    syntax_example = "ST_LineExtend(line, distance_forward, distance_backward)",
    argument(name = "line", description = "geometry"),
    argument(name = "distance_forward", description = "float8"),
    argument(name = "distance_backward", description = "float8, default 0"),
    related_udf(name = "st_project")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineExtend;

impl LineExtend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineExtend {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineExtend {
    fn name(&self) -> &str {
        "st_lineextend"
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
        Ok(line_extend_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_extend_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineExtendKernel {
        forward: optional_float_arg(&args, 1, 0.0)?,
        backward: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineExtendKernel {
    forward: Float64Array,
    backward: Float64Array,
}

impl GeometryKernel for LineExtendKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.forward.is_null(row) || self.backward.is_null(row) {
            return Ok(None);
        }
        let (forward, backward) = (self.forward.value(row), self.backward.value(row));
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "st_lineextend: Argument must be LINESTRING geometry"
            )
            .into());
        };
        if forward < 0.0 || backward < 0.0 {
            return Err(exec_datafusion_err!(
                "st_lineextend: lwline_extend: distances must be non-negative"
            )
            .into());
        }
        let dim = dimension(geom.dim());
        let (mut coords, _) = owned_line_string(line, dim).into_inner();
        if coords.is_empty() {
            return Ok(None);
        }
        let end = beyond(coords.iter().rev(), forward);
        let start = beyond(coords.iter(), backward);
        if let Some(start) = start {
            coords.insert(0, start);
        }
        if let Some(end) = end {
            coords.push(end);
        }
        Ok(Some(Wkt::LineString(LineString::new(coords, dim))))
    }
}

/// The point `distance` beyond the first of `coords`, continuing the direction from the first
/// coordinate that differs from it in 2D; `None` for no distance or no such coordinate.
fn beyond<'a>(
    mut coords: impl Iterator<Item = &'a Coord<f64>>,
    distance: f64,
) -> Option<Coord<f64>> {
    let end = coords.next()?;
    let from = coords.find(|coord| coord.x != end.x || coord.y != end.y)?;
    if distance == 0.0 {
        return None;
    }
    let length = (end.x - from.x).hypot(end.y - from.y);
    let extend = |end: f64, from: f64| end + (end - from) / length * distance;
    Some(Coord {
        x: extend(end.x, from.x),
        y: extend(end.y, from.y),
        z: end.z.zip(from.z).map(|(end, from)| extend(end, from)),
        m: end.m.zip(from.m).map(|(end, from)| extend(end, from)),
    })
}
