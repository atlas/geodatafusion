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

use crate::error::GeoDataFusionResult;
use crate::udf::native::linear_referencing::util::locate::{
    Ordinate, has_ordinate, locate_between,
};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LocateBetweenElevations(geometry geom, float8 elevation_start, float8
/// elevation_end).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Float, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "elevation_start", "elevation_end"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the portions of a geometry that lie in an elevation (Z) range.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the parts of a geometry with Z whose Z is between elevation_start and elevation_end, either way round, edges included: the points of a point or multipoint as a MULTIPOINT, the pieces of lines as a MULTILINESTRING (a GEOMETRYCOLLECTION if a line only touches the range at a vertex, which gives a point), polygons clipped ring by ring as a MULTIPOLYGON, and the parts of a collection's members as a GEOMETRYCOLLECTION. Crossings are interpolated, M included; on lines, they can differ from PostGIS's in the last digit. Geometries without Z are errors, as in PostGIS.",
    syntax_example = "ST_LocateBetweenElevations(geom, elevation_start, elevation_end)",
    argument(name = "geom", description = "geometry"),
    argument(name = "elevation_start", description = "float8"),
    argument(name = "elevation_end", description = "float8"),
    related_udf(name = "st_locatebetween")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LocateBetweenElevations;

impl LocateBetweenElevations {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocateBetweenElevations {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LocateBetweenElevations {
    fn name(&self) -> &str {
        "st_locatebetweenelevations"
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
        Ok(locate_between_elevations_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn locate_between_elevations_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = LocateBetweenElevationsKernel {
        from: optional_float_arg(&args, 1, 0.0)?,
        to: optional_float_arg(&args, 2, 0.0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct LocateBetweenElevationsKernel {
    from: Float64Array,
    to: Float64Array,
}

impl GeometryKernel for LocateBetweenElevationsKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.from.is_null(row) || self.to.is_null(row) {
            return Ok(None);
        }
        if !has_ordinate(geom, Ordinate::Z) {
            return Err(exec_datafusion_err!(
                "st_locatebetweenelevations: This function only accepts geometries with Z dimensions."
            )
            .into());
        }
        Ok(Some(locate_between(
            &to_owned_geometry(geom),
            Ordinate::Z,
            self.from.value(row),
            self.to.value(row),
        )))
    }
}
