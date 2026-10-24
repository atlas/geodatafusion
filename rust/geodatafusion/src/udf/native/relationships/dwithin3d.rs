//! ST_3DDWithin, ST_3DDFullyWithin and ST_3DIntersects.

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
use crate::udf::native::util::distance3d::{distance_3d, max_distance_3d};
use crate::util::args::optional_float_arg;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_3DDWithin(geometry g1, geometry g2, double precision distance_of_srid), and the
/// same for ST_3DDFullyWithin.
static WITHIN_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry, Arg::Float]];

static WITHIN_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2", "distance_of_srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS: ST_3DIntersects(geometry geomA, geometry geomB).
static INTERSECTS_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static INTERSECTS_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geomA", "geomB"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Tests if two 3D geometries are within a given 3D distance.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Returns true if the 3D distance between two geometries (see ST_3DDistance) is at most distance_of_srid, in the units of the spatial reference system. If either geometry has no Z, the 2D distance is used. Returns false if either geometry is empty. A negative distance is an error.",
    syntax_example = "ST_3DDWithin(g1, g2, distance_of_srid)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    argument(name = "distance_of_srid", description = "double precision"),
    related_udf(name = "st_3ddistance"),
    related_udf(name = "st_dwithin"),
    related_udf(name = "st_3ddfullywithin")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DWithin3D;

impl DWithin3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DWithin3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DWithin3D {
    fn name(&self) -> &str {
        "st_3ddwithin"
    }

    fn signature(&self) -> &Signature {
        &WITHIN_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, WITHIN_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(within_impl(self.name(), args, Test::Within)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Tests if two 3D geometries are entirely within a given 3D distance.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Returns true if every point of each geometry is within distance_of_srid of the other: if their 3D maximum distance (see ST_3DMaxDistance) is at most distance_of_srid. If either geometry has no Z, the 2D maximum distance is used. Returns false if either geometry is empty. A negative distance is an error.",
    syntax_example = "ST_3DDFullyWithin(g1, g2, distance_of_srid)",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry"),
    argument(name = "distance_of_srid", description = "double precision"),
    related_udf(name = "st_3dmaxdistance"),
    related_udf(name = "st_3ddwithin")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DFullyWithin3D;

impl DFullyWithin3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DFullyWithin3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DFullyWithin3D {
    fn name(&self) -> &str {
        "st_3ddfullywithin"
    }

    fn signature(&self) -> &Signature {
        &WITHIN_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, WITHIN_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(within_impl(self.name(), args, Test::FullyWithin)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Tests if two geometries spatially intersect in 3D.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Returns true if two geometries share a point in 3D: if their 3D distance (see ST_3DDistance) is 0. If either geometry has no Z, it is whether they intersect in 2D. Returns false if either geometry is empty.",
    syntax_example = "ST_3DIntersects(geomA, geomB)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "geomB", description = "geometry"),
    related_udf(name = "st_intersects"),
    related_udf(name = "st_3ddistance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Intersects3D;

impl Intersects3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Intersects3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Intersects3D {
    fn name(&self) -> &str {
        "st_3dintersects"
    }

    fn signature(&self) -> &Signature {
        &INTERSECTS_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, INTERSECTS_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(within_impl(self.name(), args, Test::Intersects)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Which relationship to test.
#[derive(Debug, Clone, Copy)]
enum Test {
    /// The minimum distance is at most the tolerance.
    Within,
    /// The maximum distance is at most the tolerance.
    FullyWithin,
    /// The minimum distance is 0.
    Intersects,
}

fn within_impl(
    name: &str,
    args: ScalarFunctionArgs,
    test: Test,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = WithinKernel {
        name: name.to_string(),
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        tolerance: optional_float_arg(&args, 2, 0.0)?,
        test,
    };
    let result: BooleanArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct WithinKernel {
    name: String,
    geom2: OwnedColumn,
    tolerance: Float64Array,
    test: Test,
}

impl GeometryKernel for WithinKernel {
    type Output = bool;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // STRICT: SQL NULL in any argument gives SQL NULL.
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        if self.tolerance.is_null(row) {
            return Ok(None);
        }
        let tolerance = self.tolerance.value(row);
        if tolerance < 0.0 {
            return Err(
                exec_datafusion_err!("{}: Tolerance cannot be less than zero", self.name).into(),
            );
        }
        // EMPTY is within no distance of anything.
        let distance = match self.test {
            Test::Within | Test::Intersects => distance_3d(geom1, geom2),
            Test::FullyWithin => max_distance_3d(geom1, geom2),
        };
        Ok(Some(distance.is_some_and(|distance| distance <= tolerance)))
    }
}
