use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::udf::native::io::util::number::{DEFAULT_MAX_DECIMAL_DIGITS, write_number};
use crate::util::args::optional_int_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AsSVG(geometry geom, integer rel=0, integer maxdecimaldigits=15). The geography
/// form waits for the geography type.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "rel", "maxdecimaldigits"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns SVG path data for a geometry.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the geometry as SVG path data, with y negated, as SVG's y points down: cx and cy attributes for a point (x and y with rel), and M, L and Z commands for lines and polygon rings (M, then relative l and z moves with rel non-zero). Coordinates have at most maxdecimaldigits decimals. Points of a multipoint are separated by commas, the parts of other multi geometries by spaces, and the members of a collection by semicolons; nested collections are an error, as in PostGIS. An empty geometry gives the empty string. Z and M are ignored. The geography form isn't supported yet.",
    syntax_example = "ST_AsSVG(geom, rel, maxdecimaldigits)",
    argument(name = "geom", description = "geometry"),
    argument(name = "rel", description = "integer, default 0"),
    argument(name = "maxdecimaldigits", description = "integer, default 15")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsSVG;

impl AsSVG {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsSVG {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsSVG {
    fn name(&self) -> &str {
        "st_assvg"
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
        Ok(as_svg_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_svg_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsSVGKernel {
        rel: optional_int_arg(&args, 1, 0)?,
        decimals: optional_int_arg(&args, 2, DEFAULT_MAX_DECIMAL_DIGITS)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsSVGKernel {
    rel: Int32Array,
    decimals: Int32Array,
}

impl GeometryKernel for AsSVGKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        if self.rel.is_null(row) || self.decimals.is_null(row) {
            return Ok(None);
        }
        let svg = Svg {
            relative: self.rel.value(row) != 0,
            decimals: self.decimals.value(row),
        };
        let mut out = String::new();
        // An empty geometry, all the way down, gives the empty string.
        if !is_geometry_topologically_empty(geom) {
            svg.geometry(&mut out, geom, true)?;
        }
        Ok(Some(out))
    }
}

struct Svg {
    relative: bool,
    decimals: i32,
}

impl Svg {
    /// Writes a geometry; `top` is false for a collection's members, which can't be collections.
    fn geometry(
        &self,
        out: &mut String,
        geom: &impl GeometryTrait<T = f64>,
        top: bool,
    ) -> GeoDataFusionResult<()> {
        match geom.as_type() {
            GeometryType::Point(point) => self.point(out, point),
            GeometryType::LineString(line) => self.line(out, line),
            GeometryType::Polygon(polygon) => self.polygon(out, polygon),
            // The parts of multi geometries are separated even when empty.
            GeometryType::MultiPoint(points) => {
                for (index, point) in points.points().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    self.point(out, &point);
                }
            }
            GeometryType::MultiLineString(lines) => {
                for (index, line) in lines.line_strings().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    self.line(out, &line);
                }
            }
            GeometryType::MultiPolygon(polygons) => {
                for (index, polygon) in polygons.polygons().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    self.polygon(out, &polygon);
                }
            }
            GeometryType::GeometryCollection(collection) => {
                if !top {
                    return Err(exec_datafusion_err!(
                        "st_assvg: assvg_geom_buf: 'GeometryCollection' geometry type not supported."
                    )
                    .into());
                }
                // A collection's empty members are left out, separators and all.
                let mut first = true;
                for member in collection.geometries() {
                    if is_geometry_topologically_empty(&member) {
                        continue;
                    }
                    if !first {
                        out.push(';');
                    }
                    first = false;
                    self.geometry(out, &member, false)?;
                }
            }
            GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {}
        }
        Ok(())
    }

    /// The value rounded to the decimals, halves away from zero.
    fn round(&self, value: f64) -> f64 {
        let scale = 10f64.powi(self.decimals.max(0));
        (value * scale).round() / scale
    }

    fn number(&self, out: &mut String, value: f64) {
        write_number(out, value, self.decimals);
    }

    fn point(&self, out: &mut String, point: &impl PointTrait<T = f64>) {
        let Some(coord) = point.coord() else {
            return;
        };
        let (x, y) = if self.relative {
            ("x", "y")
        } else {
            ("cx", "cy")
        };
        out.push_str(x);
        out.push_str("=\"");
        self.number(out, coord.x());
        out.push_str("\" ");
        out.push_str(y);
        out.push_str("=\"");
        self.number(out, -coord.y());
        out.push('"');
    }

    /// Writes `M` and the first point, then `L` (or `l`) and the rest.
    ///
    /// Relative moves are between coordinates rounded to the decimals, and from where the moves
    /// so far add up to, rounding errors included, as PostGIS's output shows.
    fn path(&self, out: &mut String, coords: &[(f64, f64)]) {
        out.push('M');
        let Some(&(x0, y0)) = coords.first() else {
            out.push(' ');
            return;
        };
        let mut position = if self.relative {
            (self.round(x0), self.round(y0))
        } else {
            (x0, y0)
        };
        out.push(' ');
        self.number(out, position.0);
        out.push(' ');
        self.number(out, -position.1);
        if coords.len() < 2 {
            return;
        }
        out.push_str(if self.relative { " l" } else { " L" });
        for &(x, y) in &coords[1..] {
            let (dx, dy) = if self.relative {
                let delta = (self.round(x) - position.0, self.round(y) - position.1);
                position = (position.0 + delta.0, position.1 + delta.1);
                delta
            } else {
                (x, y)
            };
            out.push(' ');
            self.number(out, dx);
            out.push(' ');
            self.number(out, -dy);
        }
    }

    fn line(&self, out: &mut String, line: &impl LineStringTrait<T = f64>) {
        let coords: Vec<(f64, f64)> = line.coords().map(|c| (c.x(), c.y())).collect();
        self.path(out, &coords);
    }

    /// Writes each ring without its closing point, and `Z` (or `z`).
    fn polygon(&self, out: &mut String, polygon: &impl PolygonTrait<T = f64>) {
        let rings = polygon.exterior().into_iter().chain(polygon.interiors());
        for (index, ring) in rings.enumerate() {
            if index > 0 {
                out.push(' ');
            }
            let mut coords: Vec<(f64, f64)> = ring.coords().map(|c| (c.x(), c.y())).collect();
            coords.pop();
            self.path(out, &coords);
            out.push_str(if self.relative { " z" } else { " Z" });
        }
    }
}
