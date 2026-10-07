//! ST_DWithin.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, BooleanArray, Float64Array};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::measurement::distance::distance;
use crate::util::args::optional_float_arg;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_DWithin(geometry g1, geometry g2, double precision distance_of_srid).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry, Arg::Float]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2", "distance_of_srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns whether two geometries are within a distance of each other.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Returns true if the geometries are within a given distance of each other: if their minimum 2D Cartesian distance (see ST_Distance) is at most distance_of_srid, in the units of the spatial reference system. Returns false if either geometry is empty. A negative distance is an error.",
    syntax_example = "ST_DWithin(g1, g2, distance_of_srid)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    argument(name = "distance_of_srid", description = "double precision")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DWithin;

impl DWithin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DWithin {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DWithin {
    fn name(&self) -> &str {
        "st_dwithin"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dwithin_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dwithin_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DWithinKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        tolerance: optional_float_arg(&args, 2, 0.0)?,
    };
    let result: BooleanArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DWithinKernel {
    geom2: OwnedColumn,
    tolerance: Float64Array,
}

impl GeometryKernel for DWithinKernel {
    type Output = bool;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // ST_DWithin is STRICT: SQL NULL in any argument gives SQL NULL.
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        if self.tolerance.is_null(row) {
            return Ok(None);
        }
        let tolerance = self.tolerance.value(row);
        if tolerance < 0.0 {
            return Err(
                exec_datafusion_err!("st_dwithin: Tolerance cannot be less than zero").into(),
            );
        }
        // EMPTY is within no distance of anything.
        Ok(Some(
            distance(geom1, geom2).is_some_and(|distance| distance <= tolerance),
        ))
    }
}
