//! ST_SharedPaths.

use std::sync::LazyLock;

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

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, from_geos, has_z, to_geos};
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_SharedPaths(geometry geom1, geometry geom2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the paths two lineal geometries share.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a collection of the paths shared by two lineal geometries: a multilinestring of the paths going the same direction, and one of the paths going the opposite direction. Non-lineal inputs are an error. This function keeps Z and drops M.",
    syntax_example = "ST_SharedPaths(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SharedPaths;

impl SharedPaths {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SharedPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SharedPaths {
    fn name(&self) -> &str {
        "st_sharedpaths"
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
        Ok(shared_paths_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn shared_paths_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata("st_sharedpaths", &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = SharedPathsKernel {
        other: GeosColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SharedPathsKernel {
    other: GeosColumn,
}

impl GeometryKernel for SharedPathsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_SharedPaths is STRICT: SQL NULL in any argument gives SQL NULL.
        let Some((other_input, other)) = self.other.get(row) else {
            return Ok(None);
        };
        // PostGIS keeps a Z from GEOS only when an input has Z.
        let want_z = has_z(geom) || has_z(other_input);
        Ok(Some(from_geos(
            &to_geos(geom)?.shared_paths(other)?,
            want_z,
        )?))
    }
}
