use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::{Coord, LineString, MultiLineString, MultiPolygon, Polygon};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_RemoveSmallParts(geometry geom, double precision minSizeX, double precision
/// minSizeY). PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float, Arg::Float]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Removes small parts (polygon rings or linestrings) of a geometry.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry without the linestrings and polygon rings whose bounding box is narrower than minSizeX or lower than minSizeY. A polygon whose exterior ring is removed is removed too, and so becomes empty if it is the whole geometry. Points and GEOMETRYCOLLECTIONs are returned unchanged, as in PostGIS.",
    syntax_example = "ST_RemoveSmallParts(geom, minSizeX, minSizeY)",
    argument(name = "geom", description = "geometry"),
    argument(name = "minSizeX", description = "float8"),
    argument(name = "minSizeY", description = "float8"),
    related_udf(name = "st_simplify")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RemoveSmallParts;

impl RemoveSmallParts {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RemoveSmallParts {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RemoveSmallParts {
    fn name(&self) -> &str {
        "st_removesmallparts"
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
        Ok(remove_small_parts_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn remove_small_parts_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = RemoveSmallPartsKernel {
        min_x: optional_float_arg(&args, 1, 0.0)?,
        min_y: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct RemoveSmallPartsKernel {
    min_x: Float64Array,
    min_y: Float64Array,
}

impl GeometryKernel for RemoveSmallPartsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.min_x.is_null(row) || self.min_y.is_null(row) {
            return Ok(None);
        }
        let (min_x, min_y) = (self.min_x.value(row), self.min_y.value(row));
        // A part is small if its box is too narrow or too low.
        let small = |coords: &[Coord<f64>]| {
            let (mut xmin, mut ymin) = (f64::INFINITY, f64::INFINITY);
            let (mut xmax, mut ymax) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for coord in coords {
                (xmin, xmax) = (xmin.min(coord.x), xmax.max(coord.x));
                (ymin, ymax) = (ymin.min(coord.y), ymax.max(coord.y));
            }
            xmax - xmin < min_x || ymax - ymin < min_y
        };
        let polygon = |polygon: Polygon<f64>| {
            let (rings, dim) = polygon.into_inner();
            let mut rings = rings.into_iter();
            match rings.next() {
                Some(exterior) if !small(exterior.coords()) => Some(Polygon::new(
                    std::iter::once(exterior)
                        .chain(rings.filter(|ring| !small(ring.coords())))
                        .collect(),
                    dim,
                )),
                _ => None,
            }
        };
        Ok(Some(match to_owned_geometry(geom) {
            Wkt::LineString(line) if small(line.coords()) => {
                Wkt::LineString(LineString::new(vec![], line.dimension()))
            }
            Wkt::Polygon(p) => {
                let dim = p.dimension();
                Wkt::Polygon(polygon(p).unwrap_or(Polygon::new(vec![], dim)))
            }
            Wkt::MultiLineString(lines) => {
                let (lines, dim) = lines.into_inner();
                Wkt::MultiLineString(MultiLineString::new(
                    lines
                        .into_iter()
                        .filter(|line| !small(line.coords()))
                        .collect(),
                    dim,
                ))
            }
            Wkt::MultiPolygon(polygons) => {
                let (polygons, dim) = polygons.into_inner();
                Wkt::MultiPolygon(MultiPolygon::new(
                    polygons.into_iter().filter_map(polygon).collect(),
                    dim,
                ))
            }
            other => other,
        }))
    }
}
