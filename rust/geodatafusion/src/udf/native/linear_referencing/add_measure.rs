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
use wkt::types::{Coord, Dimension, LineString, MultiLineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::linear_referencing::util::walk::{Length, line_length};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AddMeasure(geometry geom_mline, float8 measure_start, float8 measure_end).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom_mline", "measure_start", "measure_end"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Interpolates measures along a linear geometry.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the LINESTRING or MULTILINESTRING with M values running linearly from measure_start to measure_end along its 2D length, replacing any it had; the gaps between the lines of a MULTILINESTRING don't count. A line of no length gets measures spread evenly over its points.",
    syntax_example = "ST_AddMeasure(geom_mline, measure_start, measure_end)",
    argument(name = "geom_mline", description = "geometry"),
    argument(name = "measure_start", description = "float8"),
    argument(name = "measure_end", description = "float8"),
    related_udf(name = "st_locatealong"),
    related_udf(name = "st_interpolatepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AddMeasure;

impl AddMeasure {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AddMeasure {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AddMeasure {
    fn name(&self) -> &str {
        "st_addmeasure"
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
        Ok(add_measure_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn add_measure_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AddMeasureKernel {
        start: optional_float_arg(&args, 1, 0.0)?,
        end: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct AddMeasureKernel {
    start: Float64Array,
    end: Float64Array,
}

impl GeometryKernel for AddMeasureKernel {
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
        let in_dim = dimension(geom.dim());
        let out_dim = match in_dim {
            Dimension::XYZ | Dimension::XYZM => Dimension::XYZM,
            Dimension::XY | Dimension::XYM => Dimension::XYM,
        };
        let lines: Vec<Vec<Coord<f64>>> = match geom.as_type() {
            GeometryType::LineString(line) => vec![owned_line_string(line, in_dim).into_inner().0],
            GeometryType::MultiLineString(lines) => lines
                .line_strings()
                .map(|line| owned_line_string(&line, in_dim).into_inner().0)
                .collect(),
            _ => {
                return Err(exec_datafusion_err!(
                    "st_addmeasure: Only LINESTRING and MULTILINESTRING are supported"
                )
                .into());
            }
        };
        let total: f64 = lines
            .iter()
            .map(|line| line_length(line, Length::Planar))
            .sum();
        let mut walked = 0.0;
        let measured: Vec<LineString<f64>> = lines
            .into_iter()
            .map(|coords| {
                let count = coords.len();
                let mut previous: Option<Coord<f64>> = None;
                let coords = coords
                    .into_iter()
                    .enumerate()
                    .map(|(index, coord)| {
                        if let Some(previous) = previous {
                            walked += (coord.x - previous.x).hypot(coord.y - previous.y);
                        }
                        previous = Some(coord);
                        // A line of no length spreads the measures over its points.
                        let fraction = if total > 0.0 {
                            walked / total
                        } else if count > 1 {
                            index as f64 / (count - 1) as f64
                        } else {
                            0.0
                        };
                        Coord {
                            m: Some(start + (end - start) * fraction),
                            ..coord
                        }
                    })
                    .collect();
                LineString::new(coords, out_dim)
            })
            .collect();
        Ok(Some(match geom.as_type() {
            GeometryType::LineString(_) => match measured.into_iter().next() {
                Some(line) => Wkt::LineString(line),
                None => Wkt::LineString(LineString::new(vec![], out_dim)),
            },
            _ => Wkt::MultiLineString(MultiLineString::new(measured, out_dim)),
        }))
    }
}
