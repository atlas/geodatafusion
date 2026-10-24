//! The n-dimensional distance behind `<<->>`.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::distance3d::distance_nd;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: geometry_distance_centroid_nd(geometry, geometry).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the n-D distance between two geometries.
#[user_doc(
    doc_section(label = "Operators"),
    description = "Returns the n-D distance between two geometries: the 3D distance (see ST_3DDistance) if both have Z, otherwise the 2D distance, combined with the difference in M if both have M. For points and lines M is interpolated at the nearest points; for other geometries it is the gap between the M ranges of their single-precision bounding boxes, as in PostGIS. Returns 0 if either geometry is empty. A box3d is its surface.",
    syntax_example = "geometry_distance_centroid_nd(geom1, geom2)",
    alternative_syntax = "geom1 <<->> geom2",
    argument(name = "geom1", description = "geometry or box"),
    argument(name = "geom2", description = "geometry or box")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeometryDistanceCentroidNd;

impl GeometryDistanceCentroidNd {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeometryDistanceCentroidNd {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeometryDistanceCentroidNd {
    fn name(&self) -> &str {
        "geometry_distance_centroid_nd"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(distance_nd_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_nd_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DistanceNdKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceNdKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for DistanceNdKernel {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self.geom2.get(row).map(|geom2| distance_nd(geom1, geom2)))
    }
}
