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
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, MultiLineString};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{BufferStyle, from_geos, to_geos};
use crate::udf::native::linear_referencing::util::locate::{
    Ordinate, has_ordinate, locate_along, locate_between,
};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{owned_atoms, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LocateBetween(geometry geom, float8 measure_start, float8 measure_end, float8 offset
/// = 0).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Float],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "measure_start", "measure_end", "offset"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the portions of a geometry that match a measure range.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the parts of a geometry with M whose M is between measure_start and measure_end, either way round, edges included: the points of a point or multipoint as a MULTIPOINT, the pieces of lines as a MULTILINESTRING (a GEOMETRYCOLLECTION if a line only touches the range at a vertex, which gives a point), polygons clipped ring by ring as a MULTIPOLYGON, and the parts of a collection's members as a GEOMETRYCOLLECTION. Crossings are interpolated, Z included; on lines, they can differ from PostGIS's in the last digit. If the measures are equal, the result is ST_LocateAlong's. A non-zero offset replaces the pieces of lines with their 2D offset curves, to the left for a positive offset, as a MULTILINESTRING; as in PostGIS, points are left out then, and polygons are an error. Geometries without M are errors.",
    syntax_example = "ST_LocateBetween(geom, measure_start, measure_end, offset)",
    argument(name = "geom", description = "geometry"),
    argument(name = "measure_start", description = "float8"),
    argument(name = "measure_end", description = "float8"),
    argument(name = "offset", description = "float8, default 0"),
    related_udf(name = "st_locatealong"),
    related_udf(name = "st_locatebetweenelevations")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LocateBetween;

impl LocateBetween {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocateBetween {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LocateBetween {
    fn name(&self) -> &str {
        "st_locatebetween"
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
        Ok(locate_between_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn locate_between_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LocateBetweenKernel {
        from: optional_float_arg(&args, 1, 0.0)?,
        to: optional_float_arg(&args, 2, 0.0)?,
        offset: optional_float_arg(&args, 3, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LocateBetweenKernel {
    from: Float64Array,
    to: Float64Array,
    offset: Float64Array,
}

impl GeometryKernel for LocateBetweenKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.from.is_null(row) || self.to.is_null(row) || self.offset.is_null(row) {
            return Ok(None);
        }
        if !has_ordinate(geom, Ordinate::M) {
            return Err(exec_datafusion_err!(
                "st_locatebetween: This function only accepts geometries that have an M dimension."
            )
            .into());
        }
        let (from, to, offset) = (
            self.from.value(row),
            self.to.value(row),
            self.offset.value(row),
        );
        let geom = to_owned_geometry(geom);
        // PostGIS locates equal measures with ST_LocateAlong.
        if from == to {
            return Ok(Some(locate_along("st_locatebetween", geom, from, offset)?));
        }
        let located = locate_between(&geom, Ordinate::M, from, to);
        let parts = owned_atoms(&located);
        if offset == 0.0 || parts.is_empty() {
            return Ok(Some(located));
        }
        let mut lines = vec![];
        for part in &parts {
            match part {
                Wkt::LineString(_) => {
                    let style = BufferStyle::default();
                    let curve = to_geos(part)?.offset_curve(
                        offset,
                        style.quad_segs,
                        style.join,
                        style.mitre_limit,
                    )?;
                    // An empty offset stays, as an empty line.
                    for atom in owned_atoms(&from_geos(&curve, false)?) {
                        if let Wkt::LineString(line) = atom {
                            lines.push(line);
                        }
                    }
                }
                // PostGIS skips points with a notice that it can't offset them.
                Wkt::Point(_) => {}
                _ => {
                    return Err(exec_datafusion_err!(
                        "st_locatebetween: lwgeom_clip_to_ordinate_range found an unexpected type (Polygon) in the offset routine"
                    )
                    .into());
                }
            }
        }
        Ok(Some(Wkt::MultiLineString(MultiLineString::new(
            lines,
            Dimension::XY,
        ))))
    }
}
