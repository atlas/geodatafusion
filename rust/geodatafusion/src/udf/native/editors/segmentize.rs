use std::cell::Cell;
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
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodesic::Geodesic;
use crate::udf::native::util::geodetic_distance::segmentize as geodetic_segmentize;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometries_from_array, geometry_array, input_metadata, wkb_return_field};
use crate::util::geography::{geography_argument, geography_spheroid, is_geography};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{LinePart, map_line_strings};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Segmentize(geometry geom, float max_segment_length) and ST_Segmentize(geography
/// geog, float max_segment_length).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "max_segment_length"])
        .expect("parameter names are valid for a user-defined signature")
});

/// The most points one call may add to a segment.
const MAX_SEGMENTS: f64 = 2_147_483_647.0;

/// Returns a modified geometry having no segment longer than a distance.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry with each segment of its lines and rings split into equal parts no longer than max_segment_length, measured in 2D, interpolating Z and M. A repeated point is dropped, but a line keeps two. max_segment_length must be positive. For geography, max_segment_length is in metres on the sphere of the SRID's spheroid's mean radius, and, as in PostGIS, each edge is bisected at its great-circle midpoint, and each half again, until no part is longer; an edge between antipodes is an error.",
    syntax_example = "ST_Segmentize(geom, max_segment_length)",
    argument(name = "geom", description = "geometry"),
    argument(name = "max_segment_length", description = "float8"),
    related_udf(name = "st_lineinterpolatepoints")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Segmentize;

impl Segmentize {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Segmentize {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Segmentize {
    fn name(&self) -> &str {
        "st_segmentize"
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
        Ok(segmentize_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn segmentize_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    if is_geography(&args.arg_fields[0]) {
        return geography_segmentize_impl(args);
    }
    let geometries = geometry_array(&args, 0)?;
    let kernel = SegmentizeKernel {
        max_length: optional_float_arg(&args, 1, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SegmentizeKernel {
    max_length: Float64Array,
}

impl GeometryKernel for SegmentizeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.max_length.is_null(row) {
            return Ok(None);
        }
        let max_length = self.max_length.value(row);
        if max_length <= 0.0 {
            return Err(exec_datafusion_err!(
                "st_segmentize: invalid max_distance {max_length} (must be >= 0)"
            )
            .into());
        }
        // The mapping can't fail, so it notes a segment that would need too many parts.
        let too_many = Cell::new(false);
        let result = map_line_strings(geom, &|_: LinePart, coords| {
            segmentize(coords, max_length).unwrap_or_else(|| {
                too_many.set(true);
                vec![]
            })
        });
        if too_many.get() {
            return Err(exec_datafusion_err!("st_segmentize: Too many segments required").into());
        }
        Ok(Some(result))
    }
}

/// The geography form: edges bisected along great circles.
fn geography_segmentize_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    const NAME: &str = "st_segmentize";
    let (value, field, srid) = geography_argument(NAME, &args, 0)?;
    let geometries = geometries_from_array(&value.to_array(args.number_rows)?, &field)?;
    let kernel = GeographySegmentizeKernel {
        max_length: optional_float_arg(&args, 1, 0.0)?,
        radius: Geodesic::new(geography_spheroid(NAME, srid)?).radius(),
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct GeographySegmentizeKernel {
    max_length: Float64Array,
    radius: f64,
}

impl GeometryKernel for GeographySegmentizeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.max_length.is_null(row) {
            return Ok(None);
        }
        let max_length = self.max_length.value(row);
        if max_length <= 0.0 {
            return Err(exec_datafusion_err!(
                "st_segmentize: maximum segment length must be positive"
            )
            .into());
        }
        // The mapping can't fail, so it notes an antipodal edge.
        let error = Cell::new(None);
        let result = map_line_strings(geom, &|_: LinePart, coords| {
            geodetic_segmentize(coords, max_length, self.radius).unwrap_or_else(|e| {
                error.set(Some(e));
                vec![]
            })
        });
        if let Some(e) = error.get() {
            return Err(exec_datafusion_err!("st_segmentize: {e}").into());
        }
        Ok(Some(result))
    }
}

/// The coordinates with every segment split into equal parts no longer than `max_length`, or
/// `None` if a segment would need too many.
fn segmentize(coords: Vec<Coord<f64>>, max_length: f64) -> Option<Vec<Coord<f64>>> {
    let mut out: Vec<Coord<f64>> = Vec::with_capacity(coords.len());
    let last_index = coords.len().saturating_sub(1);
    for (index, end) in coords.into_iter().enumerate() {
        let Some(start) = out.last().copied() else {
            out.push(end);
            continue;
        };
        let length = (end.x - start.x).hypot(end.y - start.y);
        let parts = (length / max_length).ceil();
        if parts > MAX_SEGMENTS {
            return None;
        }
        if parts > 1.0 {
            // PostGIS steps by the delta divided by the parts, which rounds differently from
            // interpolating by fractions.
            let step = |a: f64, b: f64, part: f64| a + part * ((b - a) / parts);
            for part in 1..parts as i64 {
                let part = part as f64;
                out.push(Coord {
                    x: step(start.x, end.x, part),
                    y: step(start.y, end.y, part),
                    z: start.z.zip(end.z).map(|(a, b)| step(a, b, part)),
                    m: start.m.zip(end.m).map(|(a, b)| step(a, b, part)),
                });
            }
        }
        // A repeated point is dropped, unless the line would be left with one point.
        if end != start || (index == last_index && out.len() < 2) {
            out.push(end);
        }
    }
    Some(out)
}
