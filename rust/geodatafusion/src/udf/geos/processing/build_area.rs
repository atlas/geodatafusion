//! ST_BuildArea.

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
use wkt::types::{Dimension, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_BuildArea(geometry geom).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Builds the area enclosed by the linework of a geometry.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Creates an areal geometry formed by the constituent linework of the input geometry: rings become polygons, and rings inside them holes. Returns NULL if the linework doesn't enclose an area, and an empty polygon for an empty input. This function keeps Z and drops M.",
    syntax_example = "ST_BuildArea(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct BuildArea;

impl BuildArea {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BuildArea {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for BuildArea {
    fn name(&self) -> &str {
        "st_buildarea"
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
        Ok(build_area_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn build_area_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = BuildAreaKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct BuildAreaKernel;

impl GeometryKernel for BuildAreaKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        // PostGIS returns an empty polygon for EMPTY input, and NULL when no area is built.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(Wkt::Polygon(Polygon::empty(Dimension::XY))));
        }
        let area = to_geos(geom)?.build_area()?;
        if area.is_empty()? {
            return Ok(None);
        }
        Ok(Some(from_geos(&area, want_z)?))
    }
}
