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
use crate::udf::native::util::distance::distance;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Distance(geometry g1, geometry g2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "g2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the distance between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "For geometry types returns the minimum 2D Cartesian (planar) distance between two geometries, in projected units (spatial ref units). A point inside a polygon is at distance 0 from it. Returns NULL if either geometry is empty. Z and M are ignored. geometry_distance_centroid, the function behind the <-> operator, is the same function.",
    syntax_example = "ST_Distance(g1, g2)",
    alternative_syntax = "g1 <-> g2",
    argument(name = "g1", description = "geometry"),
    argument(name = "g2", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Distance {
    aliases: Vec<String>,
}

impl Distance {
    pub fn new() -> Self {
        Self {
            aliases: vec!["geometry_distance_centroid".to_string()],
        }
    }
}

impl Default for Distance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Distance {
    fn name(&self) -> &str {
        "st_distance"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
        Ok(distance_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn distance_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = DistanceKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DistanceKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for DistanceKernel {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self.geom2.get(row).and_then(|geom2| distance(geom1, geom2)))
    }
}
