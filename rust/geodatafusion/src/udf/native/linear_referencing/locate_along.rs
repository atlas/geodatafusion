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
use geo_traits::{Dimensions, GeometryTrait};
use wkt::Wkt;
use wkt::types::{Coord, Dimension, MultiPoint, Point};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LocateAlong(geometry geom_with_measure, float8 measure,
/// float8 offset = 0).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom_with_measure", "measure", "offset"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the point(s) on a geometry that match a measure value.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns, as a MULTIPOINT, the points of a geometry with M where M equals measure: interpolated along each segment of a line whose M range contains it, the midpoint of a segment whose M is the measure all along, and the points of a MULTIPOINT whose M is exactly the measure. Consecutive repeats are dropped. A non-zero offset moves points on lines that far to the left of the segment they lie on (to the right when negative). Polygons and geometries without M are errors, as in PostGIS.",
    syntax_example = "ST_LocateAlong(geom_with_measure, measure, offset)",
    argument(name = "geom_with_measure", description = "geometry"),
    argument(name = "measure", description = "float8"),
    argument(name = "offset", description = "float8, default 0"),
    related_udf(name = "st_locatebetween"),
    related_udf(name = "st_addmeasure")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LocateAlong;

impl LocateAlong {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocateAlong {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LocateAlong {
    fn name(&self) -> &str {
        "st_locatealong"
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
        Ok(locate_along_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn locate_along_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LocateAlongKernel {
        measure: optional_float_arg(&args, 1, 0.0)?,
        offset: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LocateAlongKernel {
    measure: Float64Array,
    offset: Float64Array,
}

impl GeometryKernel for LocateAlongKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.measure.is_null(row) || self.offset.is_null(row) {
            return Ok(None);
        }
        if !matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
            return Err(exec_datafusion_err!(
                "st_locatealong: Input geometry does not have a measure dimension"
            )
            .into());
        }
        let (measure, offset) = (self.measure.value(row), self.offset.value(row));
        let geom = to_owned_geometry(geom);
        let dim = match geom.dim() {
            Dimensions::Xyzm => Dimension::XYZM,
            _ => Dimension::XYM,
        };
        let coords = match geom {
            Wkt::Point(point) => point
                .coord()
                .filter(|c| c.m == Some(measure))
                .copied()
                .into_iter()
                .collect(),
            Wkt::MultiPoint(points) => points
                .points()
                .iter()
                .filter_map(|point| point.coord().filter(|c| c.m == Some(measure)).copied())
                .collect(),
            Wkt::LineString(line) => along(line.coords(), measure, offset),
            Wkt::MultiLineString(lines) => lines
                .line_strings()
                .iter()
                .flat_map(|line| along(line.coords(), measure, offset))
                .collect(),
            Wkt::Polygon(_) | Wkt::MultiPolygon(_) | Wkt::GeometryCollection(_) => {
                let type_name = match geom {
                    Wkt::Polygon(_) => "Polygon",
                    Wkt::MultiPolygon(_) => "MultiPolygon",
                    _ => "GeometryCollection",
                };
                return Err(exec_datafusion_err!(
                    "st_locatealong: Only linear geometries are supported, {type_name} provided."
                )
                .into());
            }
        };
        let points = coords
            .into_iter()
            .map(|coord| Point::new(Some(coord), dim))
            .collect();
        Ok(Some(Wkt::MultiPoint(MultiPoint::new(points, dim))))
    }
}

/// The points of a line at `measure`, each segment's in turn, without consecutive repeats.
fn along(coords: &[Coord<f64>], measure: f64, offset: f64) -> Vec<Coord<f64>> {
    let mut out: Vec<Coord<f64>> = vec![];
    for pair in coords.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let (Some(ma), Some(mb)) = (a.m, b.m) else {
            continue;
        };
        if measure < ma.min(mb) || measure > ma.max(mb) {
            continue;
        }
        // A segment at the measure all along gives its midpoint.
        let t = if ma == mb {
            0.5
        } else {
            (measure - ma) / (mb - ma)
        };
        let mix = |a: f64, b: f64| a + (b - a) * t;
        let mut point = Coord {
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            z: a.z.zip(b.z).map(|(a, b)| mix(a, b)),
            m: Some(measure),
        };
        if offset != 0.0 {
            let theta = (b.y - a.y).atan2(b.x - a.x);
            point.x -= offset * theta.sin();
            point.y += offset * theta.cos();
        }
        if out.last() != Some(&point) {
            out.push(point);
        }
    }
    out
}
