//! ST_DelaunayTriangles.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
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

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, has_z, to_geos};
use crate::util::args::{optional_float_arg, optional_int_arg};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_DelaunayTriangles(geometry g1, float8 tolerance = 0, integer flags = 0).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Integer],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "tolerance", "flags"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the Delaunay triangulation of a geometry's vertices.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns the Delaunay triangulation of the vertices of a geometry. Vertices closer than tolerance are merged. flags 0 (the default) returns a collection of triangular polygons, 1 a multilinestring of the edges; 2 (a TIN) is unsupported. Fewer than three distinct vertices give an empty collection. This function keeps Z and drops M.",
    syntax_example = "ST_DelaunayTriangles(g1, tolerance, flags)",
    argument(name = "g1", description = "geometry"),
    argument(name = "tolerance", description = "float8"),
    argument(name = "flags", description = "integer")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DelaunayTriangles;

impl DelaunayTriangles {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DelaunayTriangles {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DelaunayTriangles {
    fn name(&self) -> &str {
        "st_delaunaytriangles"
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
        Ok(delaunay_triangles_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn delaunay_triangles_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = DelaunayTrianglesKernel {
        tolerance: optional_float_arg(&args, 1, 0.0)?,
        flags: optional_int_arg(&args, 2, 0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct DelaunayTrianglesKernel {
    tolerance: Float64Array,
    flags: Int32Array,
}

impl GeometryKernel for DelaunayTrianglesKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        // ST_DelaunayTriangles is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.tolerance.is_null(row) || self.flags.is_null(row) {
            return Ok(None);
        }
        let only_edges = match self.flags.value(row) {
            0 => false,
            1 => true,
            flags => {
                return Err(exec_datafusion_err!(
                    "st_delaunaytriangles: flags {flags} (a TIN) is unsupported"
                )
                .into());
            }
        };
        let tolerance = self.tolerance.value(row);
        let triangles = to_geos(geom)?.delaunay_triangulation(tolerance, only_edges)?;
        Ok(Some(from_geos(&triangles, want_z)?))
    }
}
