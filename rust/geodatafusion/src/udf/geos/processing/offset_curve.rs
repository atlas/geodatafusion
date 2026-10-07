//! ST_OffsetCurve.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array, StringArray};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryCollectionTrait, GeometryTrait, GeometryType, MultiLineStringTrait};
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, LineString, MultiLineString};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{BufferStyle, StyleKeys, empty_like, from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::{optional_float_arg, optional_text_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_OffsetCurve(geometry line, float8 distance, text params = '').
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Text],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["line", "distance", "params"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a line at a distance from a line.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a line offset a given distance and side from an input line: to the left for a positive distance, to the right for a negative one. The lines of a multilinestring or collection are offset one by one. The optional params string holds space-separated key=value style parameters: quad_segs, join=round|mitre|miter|bevel and mitre_limit (or miter_limit). Other geometry types are an error, and an empty input is returned unchanged. The result is 2D.",
    syntax_example = "ST_OffsetCurve(line, distance, params)",
    argument(name = "line", description = "geometry"),
    argument(name = "distance", description = "float8"),
    argument(name = "params", description = "text")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct OffsetCurve;

impl OffsetCurve {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OffsetCurve {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for OffsetCurve {
    fn name(&self) -> &str {
        "st_offsetcurve"
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
        Ok(offset_curve_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn offset_curve_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = OffsetCurveKernel {
        distance: optional_float_arg(&args, 1, 0.0)?,
        params: optional_text_arg(&args, 2, "")?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct OffsetCurveKernel {
    distance: Float64Array,
    params: StringArray,
}

impl GeometryKernel for OffsetCurveKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_OffsetCurve is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.distance.is_null(row) || self.params.is_null(row) {
            return Ok(None);
        }
        let style = BufferStyle::parse(
            "st_offsetcurve",
            self.params.value(row),
            StyleKeys::OffsetCurve,
        )?;
        let distance = self.distance.value(row);
        // PostGIS returns EMPTY input unchanged, M included.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        let lines = match geom.as_type() {
            GeometryType::LineString(_) => return Ok(Some(offset(geom, distance, &style)?)),
            GeometryType::MultiLineString(lines) => lines
                .line_strings()
                .map(|line| offset(&line, distance, &style))
                .collect::<GeoDataFusionResult<Vec<_>>>()?,
            GeometryType::GeometryCollection(collection) => collection
                .geometries()
                .map(|member| match member.as_type() {
                    GeometryType::LineString(_) => offset(&member, distance, &style),
                    _ => Err(not_linear(&member)),
                })
                .collect::<GeoDataFusionResult<Vec<_>>>()?,
            _ => return Err(not_linear(geom)),
        };
        // The offsets of the lines are collected; one line is returned as it is.
        let mut parts: Vec<LineString<f64>> = vec![];
        for line in lines {
            match line {
                Wkt::LineString(line) if line.coords().is_empty() => {}
                Wkt::LineString(line) => parts.push(line),
                Wkt::MultiLineString(lines) => parts.extend(lines.into_inner().0),
                _ => {}
            }
        }
        Ok(Some(if parts.len() == 1 {
            Wkt::LineString(parts.remove(0))
        } else {
            Wkt::MultiLineString(MultiLineString::new(parts, Dimension::XY))
        }))
    }
}

/// The offset curve of one linestring, from GEOS.
fn offset(
    line: &impl GeometryTrait<T = f64>,
    distance: f64,
    style: &BufferStyle,
) -> GeoDataFusionResult<Wkt<f64>> {
    let curve =
        to_geos(line)?.offset_curve(distance, style.quad_segs, style.join, style.mitre_limit)?;
    from_geos(&curve)
}

fn not_linear(geom: &impl GeometryTrait<T = f64>) -> crate::error::GeoDataFusionError {
    let kind = match geom.as_type() {
        GeometryType::Point(_) => "Point",
        GeometryType::Polygon(_) | GeometryType::Rect(_) | GeometryType::Triangle(_) => "Polygon",
        GeometryType::MultiPoint(_) => "MultiPoint",
        GeometryType::MultiPolygon(_) => "MultiPolygon",
        GeometryType::GeometryCollection(_) => "GeometryCollection",
        _ => "LineString",
    };
    exec_datafusion_err!("st_offsetcurve: input is not linear (type {kind})").into()
}
