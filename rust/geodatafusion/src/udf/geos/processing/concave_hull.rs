//! ST_ConcaveHull.

use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::{optional_bool_arg, optional_float_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ConcaveHull(geometry param_geom, float8 param_pctconvex,
/// boolean param_allow_holes = false).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Boolean],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["param_geom", "param_pctconvex", "param_allow_holes"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a concave hull of a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Computes a possibly concave geometry that contains all the vertices of the input. param_pctconvex, between 0 and 1, controls the concaveness: 1 gives the convex hull, and smaller values more concave hulls. param_allow_holes (default false) allows holes. Polygonal input (POLYGON or MULTIPOLYGON), which PostGIS hulls as polygons rather than as vertices, is unsupported. An empty input gives an empty polygon. This function keeps Z and drops M.",
    syntax_example = "ST_ConcaveHull(param_geom, param_pctconvex, param_allow_holes)",
    argument(name = "param_geom", description = "geometry"),
    argument(name = "param_pctconvex", description = "float8"),
    argument(name = "param_allow_holes", description = "boolean")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ConcaveHull;

impl ConcaveHull {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConcaveHull {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ConcaveHull {
    fn name(&self) -> &str {
        "st_concavehull"
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
        Ok(concave_hull_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn concave_hull_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ConcaveHullKernel {
        pct_convex: optional_float_arg(&args, 1, 1.0)?,
        allow_holes: optional_bool_arg(&args, 2, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ConcaveHullKernel {
    pct_convex: Float64Array,
    allow_holes: BooleanArray,
}

impl GeometryKernel for ConcaveHullKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_ConcaveHull is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.pct_convex.is_null(row) || self.allow_holes.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns an empty polygon for EMPTY input.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(Wkt::Polygon(Polygon::empty(Dimension::XY))));
        }
        // PostGIS hulls polygonal input with GEOSConcaveHullOfPolygons, which the geos crate
        // doesn't bind. A vertex hull would differ, so refuse it.
        if matches!(
            geom.as_type(),
            GeometryType::Polygon(_) | GeometryType::MultiPolygon(_)
        ) {
            return Err(
                exec_datafusion_err!("st_concavehull: polygonal input is unsupported").into(),
            );
        }
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        let (pct_convex, allow_holes) = (self.pct_convex.value(row), self.allow_holes.value(row));
        let hull = to_geos(geom)?.concave_hull(pct_convex, allow_holes)?;
        Ok(Some(from_geos(&hull, want_z)?))
    }
}
