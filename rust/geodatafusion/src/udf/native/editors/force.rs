//! ST_Force2D, ST_Force3DZ (alias ST_Force3D), ST_Force3DM and ST_Force4D: a geometry in another
//! coordinate dimension.

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
use wkt::types::{Coord, Dimension};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_coords_to_dimension;
use crate::util::signature::{Arg, coerce_args, single_geometry};

/// PostGIS: ST_Force3DZ(geometry geom, float zvalue = 0.0), and the same for ST_Force3D.
static FORCE_3DZ_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Float]];

static FORCE_3DZ_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "zvalue"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS: ST_Force3DM(geometry geom, float mvalue = 0.0).
static FORCE_3DM_ARGUMENTS: &[&[Arg]] = FORCE_3DZ_ARGUMENTS;

static FORCE_3DM_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "mvalue"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS: ST_Force4D(geometry geom, float zvalue = 0.0, float mvalue = 0.0).
static FORCE_4D_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
];

static FORCE_4D_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "zvalue", "mvalue"])
        .expect("parameter names are valid for a user-defined signature")
});

static FORCE_3DZ_ALIASES: LazyLock<Vec<String>> = LazyLock::new(|| vec!["st_force3d".to_string()]);

/// Force the geometries into a "2-dimensional mode".
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry in 2D, dropping Z and M.",
    syntax_example = "ST_Force2D(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_force3dz"),
    related_udf(name = "st_force3dm"),
    related_udf(name = "st_force4d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Force2D;

impl Force2D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Force2D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Force2D {
    fn name(&self) -> &str {
        "st_force2d"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
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

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(force_impl(args, Dimension::XY, None, None)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Force the geometries into XYZ mode.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry in XYZ, dropping M. Coordinates without Z get zvalue (default 0). ST_Force3D is an alias.",
    syntax_example = "ST_Force3DZ(geomA, zvalue)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "zvalue", description = "float8, default 0"),
    related_udf(name = "st_force2d"),
    related_udf(name = "st_force3dm"),
    related_udf(name = "st_force4d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Force3DZ;

impl Force3DZ {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Force3DZ {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Force3DZ {
    fn name(&self) -> &str {
        "st_force3dz"
    }

    fn aliases(&self) -> &[String] {
        &FORCE_3DZ_ALIASES
    }

    fn signature(&self) -> &Signature {
        &FORCE_3DZ_SIGNATURE
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
        coerce_args(self.name(), arg_types, FORCE_3DZ_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(force_impl(args, Dimension::XYZ, Some(1), None)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Force the geometries into XYM mode.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry in XYM, dropping Z. Coordinates without M get mvalue (default 0).",
    syntax_example = "ST_Force3DM(geomA, mvalue)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "mvalue", description = "float8, default 0"),
    related_udf(name = "st_force2d"),
    related_udf(name = "st_force3dz"),
    related_udf(name = "st_force4d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Force3DM;

impl Force3DM {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Force3DM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Force3DM {
    fn name(&self) -> &str {
        "st_force3dm"
    }

    fn signature(&self) -> &Signature {
        &FORCE_3DM_SIGNATURE
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
        coerce_args(self.name(), arg_types, FORCE_3DM_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(force_impl(args, Dimension::XYM, None, Some(1))?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Force the geometries into XYZM mode.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry in XYZM. Coordinates without Z get zvalue, and without M mvalue (both default 0).",
    syntax_example = "ST_Force4D(geomA, zvalue, mvalue)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "zvalue", description = "float8, default 0"),
    argument(name = "mvalue", description = "float8, default 0"),
    related_udf(name = "st_force2d"),
    related_udf(name = "st_force3dz"),
    related_udf(name = "st_force3dm")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Force4D;

impl Force4D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Force4D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Force4D {
    fn name(&self) -> &str {
        "st_force4d"
    }

    fn signature(&self) -> &Signature {
        &FORCE_4D_SIGNATURE
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
        coerce_args(self.name(), arg_types, FORCE_4D_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(force_impl(args, Dimension::XYZM, Some(1), Some(2))?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Converts every geometry to `dim`, filling a missing Z from argument `zvalue` and a missing M
/// from argument `mvalue` (0 when the call doesn't have them).
fn force_impl(
    args: ScalarFunctionArgs,
    dim: Dimension,
    zvalue: Option<usize>,
    mvalue: Option<usize>,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let fill = |index: Option<usize>| match index {
        Some(index) => optional_float_arg(&args, index, 0.0).map(Some),
        None => Ok(None),
    };
    let kernel = ForceKernel {
        dim,
        zvalue: fill(zvalue)?,
        mvalue: fill(mvalue)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// The target dimension, and the fill values for the ordinates it adds.
struct ForceKernel {
    dim: Dimension,
    zvalue: Option<Float64Array>,
    mvalue: Option<Float64Array>,
}

impl GeometryKernel for ForceKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        let fill = |values: &Option<Float64Array>| match values {
            Some(values) if values.is_null(row) => None,
            Some(values) => Some(Some(values.value(row))),
            None => Some(None),
        };
        let (Some(zvalue), Some(mvalue)) = (fill(&self.zvalue), fill(&self.mvalue)) else {
            return Ok(None);
        };
        let (keep_z, keep_m) = match self.dim {
            Dimension::XY => (false, false),
            Dimension::XYZ => (true, false),
            Dimension::XYM => (false, true),
            Dimension::XYZM => (true, true),
        };
        Ok(Some(map_coords_to_dimension(geom, self.dim, &|c| Coord {
            x: c.x,
            y: c.y,
            z: keep_z.then(|| c.z.or(zvalue).unwrap_or(0.0)),
            m: keep_m.then(|| c.m.or(mvalue).unwrap_or(0.0)),
        })))
    }
}
