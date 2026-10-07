//! ST_Buffer.

use std::sync::LazyLock;

use arrow_array::cast::AsArray;
use arrow_array::types::Int32Type;
use arrow_array::{Array, Float64Array, Int32Array, StringArray};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{BufferStyle, StyleKeys, from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::{optional_float_arg, optional_text_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Buffer(geometry geom, float8 radius, text options = '') and
/// ST_Buffer(geometry geom, float8 radius, integer quadsegs).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Text],
    &[Arg::Geometry, Arg::Float, Arg::Integer],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "radius", "options"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the area within a distance of a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a polygon covering all points within a given distance (radius) of a geometry. A negative radius shrinks polygons. The optional third argument is either the number of segments approximating a quarter circle (default 8), or a string of space-separated key=value style parameters: quad_segs, endcap=round|flat|butt|square, join=round|mitre|miter|bevel, mitre_limit (or miter_limit) and side=both|left|right for a one-sided buffer. An empty geometry gives an empty polygon. The result is 2D.",
    syntax_example = "ST_Buffer(geom, radius, options)",
    argument(name = "geom", description = "geometry"),
    argument(name = "radius", description = "float8"),
    argument(name = "options", description = "text")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Buffer;

impl Buffer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Buffer {
    fn name(&self) -> &str {
        "st_buffer"
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
        Ok(buffer_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The third argument of ST_Buffer, per row.
enum Style {
    Options(StringArray),
    QuadSegs(Int32Array),
}

fn buffer_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let style = match args.args.get(2) {
        Some(quad_segs) if args.arg_fields[2].data_type().is_integer() => Style::QuadSegs(
            quad_segs
                .cast_to(&DataType::Int32, None)?
                .to_array(args.number_rows)?
                .as_primitive::<Int32Type>()
                .clone(),
        ),
        _ => Style::Options(optional_text_arg(&args, 2, "")?),
    };
    let kernel = BufferKernel {
        radius: optional_float_arg(&args, 1, 0.0)?,
        style,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct BufferKernel {
    radius: Float64Array,
    style: Style,
}

impl GeometryKernel for BufferKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_Buffer is STRICT: SQL NULL in any argument gives SQL NULL.
        let style = match &self.style {
            Style::Options(options) if options.is_null(row) => return Ok(None),
            Style::Options(options) => {
                BufferStyle::parse("st_buffer", options.value(row), StyleKeys::Buffer)?
            }
            Style::QuadSegs(quad_segs) if quad_segs.is_null(row) => return Ok(None),
            Style::QuadSegs(quad_segs) => BufferStyle {
                quad_segs: quad_segs.value(row),
                ..Default::default()
            },
        };
        if self.radius.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns an empty polygon for EMPTY input.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(Wkt::Polygon(Polygon::empty(Dimension::XY))));
        }
        let width = style.width(self.radius.value(row));
        let buffer = to_geos(geom)?.buffer_with_params(width, &style.buffer_params()?)?;
        Ok(Some(from_geos(&buffer)?))
    }
}
