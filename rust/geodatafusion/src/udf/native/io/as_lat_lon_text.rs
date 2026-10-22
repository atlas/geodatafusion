use std::sync::{Arc, LazyLock};

use arrow_array::{Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_text_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AsLatLonText(geometry pt, text format='').
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Text]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["pt", "format"])
        .expect("parameter names are valid for a user-defined signature")
});

/// The format used for an empty one.
const DEFAULT_FORMAT: &str = "D°M'S.SSS\"C";

/// Returns the Degrees, Minutes, Seconds representation of the given point.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the latitude and then the longitude of a point as text, formatted by format: D, M and S stand for degrees, minutes and seconds, each written as a run of the letter (its count the width) optionally with a dot and more letters (their count the decimal places), and C for the compass direction (N, S, E or W). Degrees are required, seconds need minutes, and each appears at most once; other characters are copied. Without C, negative values get a minus sign. The empty format is D°M'S.SSS\"C. Coordinates beyond the range of latitude and longitude are wrapped over the poles and the antimeridian. Only points are supported.",
    syntax_example = "ST_AsLatLonText(pt, format)",
    argument(name = "pt", description = "geometry"),
    argument(name = "format", description = "text, default ''")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsLatLonText;

impl AsLatLonText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsLatLonText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsLatLonText {
    fn name(&self) -> &str {
        "st_aslatlontext"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_lat_lon_text_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_lat_lon_text_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsLatLonTextKernel {
        format: optional_text_arg(&args, 1, "")?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsLatLonTextKernel {
    format: StringArray,
}

impl GeometryKernel for AsLatLonTextKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        if self.format.is_null(row) {
            return Ok(None);
        }
        let GeometryType::Point(point) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "st_aslatlontext: Only points are supported, you tried type {}.",
                type_name(geom)
            )
            .into());
        };
        let Some(coord) = point.coord() else {
            return Err(exec_datafusion_err!(
                "st_aslatlontext: Cannot convert an empty point into formatted text."
            )
            .into());
        };
        let format = match self.format.value(row) {
            "" => DEFAULT_FORMAT,
            format => format,
        };
        let format = Format::parse(format)
            .map_err(|message| exec_datafusion_err!("st_aslatlontext: {message}"))?;
        let (lat, lon) = normalize(coord.y(), coord.x());
        Ok(Some(format!(
            "{} {}",
            format.render(lat, ['N', 'S']),
            format.render(lon, ['E', 'W'])
        )))
    }
}

fn type_name(geom: &impl GeometryTrait<T = f64>) -> &'static str {
    match geom.as_type() {
        GeometryType::Point(_) => "Point",
        GeometryType::LineString(_) | GeometryType::Line(_) => "LineString",
        GeometryType::Polygon(_) | GeometryType::Rect(_) | GeometryType::Triangle(_) => "Polygon",
        GeometryType::MultiPoint(_) => "MultiPoint",
        GeometryType::MultiLineString(_) => "MultiLineString",
        GeometryType::MultiPolygon(_) => "MultiPolygon",
        GeometryType::GeometryCollection(_) => "GeometryCollection",
    }
}

/// The latitude and longitude, wrapped into their ranges: a latitude past a pole comes back
/// down the other side, half way round the world.
///
/// The wrapping goes in steps of 360°, as PostGIS's results show: a remainder rounds
/// differently.
fn normalize(mut lat: f64, mut lon: f64) -> (f64, f64) {
    lat = step_into(lat, 270.0);
    if lat > 90.0 {
        lat = 180.0 - lat;
        lon += 180.0;
    }
    if lat < -90.0 {
        lat = -180.0 - lat;
        lon += 180.0;
    }
    (lat, step_into(lon, 180.0))
}

/// The value moved by whole turns into `-limit..=limit`.
fn step_into(mut value: f64, limit: f64) -> f64 {
    // Far out, the turns are taken at once rather than one by one.
    if value.abs() > 1e9 {
        value %= 360.0;
    }
    while value > limit {
        value -= 360.0;
    }
    while value < -limit {
        value += 360.0;
    }
    value
}

/// A part of a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Degrees,
    Minutes,
    Seconds,
    Compass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(char),
    /// A number field with its width and decimal places.
    Number(Field, usize, usize),
    Compass,
}

#[derive(Debug)]
struct Format {
    parts: Vec<Part>,
    minutes: bool,
    seconds: bool,
    compass: bool,
}

impl Format {
    /// The format, or PostGIS's message for a bad one.
    fn parse(format: &str) -> std::result::Result<Self, &'static str> {
        let chars: Vec<char> = format.chars().collect();
        let mut parts = vec![];
        let mut seen: Vec<Field> = vec![];
        let mut index = 0;
        while index < chars.len() {
            let c = chars[index];
            let field = match c {
                'D' => Field::Degrees,
                'M' => Field::Minutes,
                'S' => Field::Seconds,
                'C' => Field::Compass,
                _ => {
                    parts.push(Part::Text(c));
                    index += 1;
                    continue;
                }
            };
            if seen.contains(&field) {
                return Err(match field {
                    Field::Degrees => "Bad format, cannot include degrees (DD.DDD) more than once.",
                    Field::Minutes => "Bad format, cannot include minutes (MM.MMM) more than once.",
                    Field::Seconds => "Bad format, cannot include seconds (SS.SSS) more than once.",
                    Field::Compass => "Bad format, cannot include compass dir (C) more than once.",
                });
            }
            seen.push(field);
            if field == Field::Compass {
                parts.push(Part::Compass);
                index += 1;
                continue;
            }
            // A run of the letter and dots: letters before the first dot give the width, letters
            // after it the decimal places; further dots are swallowed.
            let (mut width, mut decimals, mut dotted) = (0, 0, false);
            while index < chars.len() && (chars[index] == c || chars[index] == '.') {
                match (chars[index] == '.', dotted) {
                    (true, _) => dotted = true,
                    (false, false) => width += 1,
                    (false, true) => decimals += 1,
                }
                index += 1;
            }
            parts.push(Part::Number(field, width, decimals));
        }
        if !seen.contains(&Field::Degrees) {
            return Err("Bad format, degrees (DD.DDD) must be included.");
        }
        let (minutes, seconds) = (
            seen.contains(&Field::Minutes),
            seen.contains(&Field::Seconds),
        );
        if seconds && !minutes {
            return Err(
                "Bad format, cannot include seconds (SS.SSS) without including minutes (MM.MMM).",
            );
        }
        Ok(Format {
            parts,
            minutes,
            seconds,
            compass: seen.contains(&Field::Compass),
        })
    }

    /// The value formatted, with `compass` the letters for positive and negative values.
    fn render(&self, value: f64, compass: [char; 2]) -> String {
        let negative = value < 0.0;
        let magnitude = value.abs();
        let (mut degrees, mut minutes, mut seconds) = (magnitude, 0.0, 0.0);
        if self.minutes {
            minutes = (degrees - degrees.floor()) * 60.0;
            degrees = degrees.floor();
        }
        if self.seconds {
            seconds = (minutes - minutes.floor()) * 60.0;
            minutes = minutes.floor();
            // Seconds that round to 60 carry into the minutes, and so on.
            let decimals = self.decimals(Field::Seconds);
            if format!("{seconds:.decimals$}")
                .parse::<f64>()
                .unwrap_or(0.0)
                >= 60.0
            {
                seconds = 0.0;
                minutes += 1.0;
                if minutes >= 60.0 {
                    minutes = 0.0;
                    degrees += 1.0;
                }
            }
        }
        // Without a compass direction, the degrees carry the sign, even when they round to 0.
        if negative && !self.compass {
            degrees = -degrees;
        }
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(c) => out.push(*c),
                Part::Compass => out.push(if negative { compass[1] } else { compass[0] }),
                Part::Number(field, width, decimals) => {
                    let value = match field {
                        Field::Degrees => degrees,
                        Field::Minutes => minutes,
                        _ => seconds,
                    };
                    out.push_str(&format!("{value:>width$.decimals$}"));
                }
            }
        }
        out
    }

    fn decimals(&self, field: Field) -> usize {
        self.parts
            .iter()
            .find_map(|part| match part {
                Part::Number(f, _, decimals) if *f == field => Some(*decimals),
                _ => None,
            })
            .unwrap_or(0)
    }
}
