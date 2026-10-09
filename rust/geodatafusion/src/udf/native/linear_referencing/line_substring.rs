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
use geo_traits::{GeometryTrait, GeometryType, MultiLineStringTrait};
use wkt::Wkt;
use wkt::types::{Coord, Dimension, GeometryCollection, LineString, MultiLineString, Point};

use crate::error::GeoDataFusionResult;
use crate::udf::native::linear_referencing::util::walk::{Length, line_length, substring};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineSubstring(geometry a_linestring, float8 startfraction, float8 endfraction).
/// The geography form waits for the geography type.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_linestring", "startfraction", "endfraction"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the part of a line between two fractional locations.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the part of a LINESTRING between startfraction and endfraction (0 to 1) of its 2D length, interpolating Z and M at the ends, without repeated points; a POINT if that leaves one. For a MULTILINESTRING the fractions are of the total length, and the result is a MULTILINESTRING of the pieces, or a GEOMETRYCOLLECTION if some piece is a point. An empty line gives NULL. The geography form isn't supported yet.",
    syntax_example = "ST_LineSubstring(a_linestring, startfraction, endfraction)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "startfraction", description = "float8 from 0 to 1"),
    argument(name = "endfraction", description = "float8 from 0 to 1"),
    related_udf(name = "st_lineinterpolatepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineSubstring;

impl LineSubstring {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineSubstring {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineSubstring {
    fn name(&self) -> &str {
        "st_linesubstring"
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
        Ok(line_substring_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_substring_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineSubstringKernel {
        start: optional_float_arg(&args, 1, 0.0)?,
        end: optional_float_arg(&args, 2, 1.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LineSubstringKernel {
    start: Float64Array,
    end: Float64Array,
}

impl GeometryKernel for LineSubstringKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.start.is_null(row) || self.end.is_null(row) {
            return Ok(None);
        }
        let (start, end) = (self.start.value(row), self.end.value(row));
        if !(0.0..=1.0).contains(&start) {
            return Err(exec_datafusion_err!(
                "st_linesubstring: line_interpolate_point: 2nd arg isn't within [0,1]"
            )
            .into());
        }
        if !(0.0..=1.0).contains(&end) {
            return Err(exec_datafusion_err!(
                "st_linesubstring: line_interpolate_point: 3rd arg isn't within [0,1]"
            )
            .into());
        }
        if start > end {
            return Err(exec_datafusion_err!(
                "st_linesubstring: 2nd arg must be smaller then 3rd arg"
            )
            .into());
        }
        let dim = dimension(geom.dim());
        match geom.as_type() {
            GeometryType::LineString(line) => {
                let (coords, _) = owned_line_string(line, dim).into_inner();
                Ok(piece(substring(&coords, start, end), dim))
            }
            GeometryType::MultiLineString(lines) => {
                let lines: Vec<Vec<Coord<f64>>> = lines
                    .line_strings()
                    .map(|line| owned_line_string(&line, dim).into_inner().0)
                    .collect();
                Ok(multi_substring(&lines, start, end, dim))
            }
            _ => Err(exec_datafusion_err!(
                "st_linesubstring: line_substring: 1st arg isn't a line"
            )
            .into()),
        }
    }
}

/// A substring as a geometry: NULL when empty, a POINT when one point is left.
fn piece(coords: Vec<Coord<f64>>, dim: Dimension) -> Option<Wkt<f64>> {
    match coords.len() {
        0 => None,
        1 => Some(Wkt::Point(Point::new(coords.into_iter().next(), dim))),
        _ => Some(Wkt::LineString(LineString::new(coords, dim))),
    }
}

/// The substring of several lines, by fractions of their total length: each line's part, if it
/// has one. A line touched only at an end gives a point, which makes the result a collection.
fn multi_substring(
    lines: &[Vec<Coord<f64>>],
    start: f64,
    end: f64,
    dim: Dimension,
) -> Option<Wkt<f64>> {
    let lengths: Vec<f64> = lines
        .iter()
        .map(|line| line_length(line, Length::Planar))
        .collect();
    let total: f64 = lengths.iter().sum();
    let (from, to) = (start * total, end * total);
    let mut pieces = vec![];
    let mut offset = 0.0;
    for (line, length) in lines.iter().zip(lengths) {
        let (line_from, line_to) = ((from - offset) / length, (to - offset) / length);
        offset += length;
        if length == 0.0 || line_to < 0.0 || line_from > 1.0 {
            continue;
        }
        let part = substring(line, line_from.max(0.0), line_to.min(1.0));
        pieces.extend(piece(part, dim));
    }
    if pieces
        .iter()
        .all(|piece| matches!(piece, Wkt::LineString(_)))
    {
        let lines = pieces
            .into_iter()
            .filter_map(|piece| match piece {
                Wkt::LineString(line) => Some(line),
                _ => None,
            })
            .collect();
        Some(Wkt::MultiLineString(MultiLineString::new(lines, dim)))
    } else {
        Some(Wkt::GeometryCollection(GeometryCollection::new(
            pieces, dim,
        )))
    }
}
