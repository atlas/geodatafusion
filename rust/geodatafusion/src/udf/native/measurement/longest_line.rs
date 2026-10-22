//! ST_LongestLine and ST_MaxDistance: the farthest points of two geometries.

use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
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
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::distance::farthest;
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LongestLine(geometry geom1, geometry geom2), and the same for ST_MaxDistance.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the 2D longest line between two geometries.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D line between the farthest points of two geometries, from geom1 to geom2; its length is ST_MaxDistance. The points are vertices, of a polygon's shell. If several pairs are equally far, the first found is returned. Returns NULL if either geometry is empty. The result is 2D.",
    syntax_example = "ST_LongestLine(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_maxdistance"),
    related_udf(name = "st_shortestline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LongestLine;

impl LongestLine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LongestLine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LongestLine {
    fn name(&self) -> &str {
        "st_longestline"
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
        Ok(longest_line_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 2D largest distance between two geometries in projected units.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the largest 2D Cartesian distance between two geometries, in projected units: the distance between their farthest vertices. Returns NULL if either geometry is empty.",
    syntax_example = "ST_MaxDistance(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_longestline"),
    related_udf(name = "st_distance")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MaxDistance;

impl MaxDistance {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MaxDistance {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MaxDistance {
    fn name(&self) -> &str {
        "st_maxdistance"
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
        Ok(max_distance_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn longest_line_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = FarthestKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

fn max_distance_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = MaxDistanceKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct FarthestKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for FarthestKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        Ok(farthest(geom1, geom2).map(|measure| {
            let coords = measure
                .points
                .into_iter()
                .map(|(x, y)| Coord {
                    x,
                    y,
                    z: None,
                    m: None,
                })
                .collect();
            Wkt::LineString(LineString::new(coords, Dimension::XY))
        }))
    }
}

struct MaxDistanceKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for MaxDistanceKernel {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self
            .geom2
            .get(row)
            .and_then(|geom2| farthest(geom1, geom2))
            .map(|measure| measure.distance))
    }
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_longest_line_returns_wkb_with_input_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(LongestLine.into());
        ctx.register_udf(GeomFromText::default().into());

        let sql = "SELECT ST_LongestLine(ST_GeomFromText('POINT(0 0)', 3857), \
                   ST_GeomFromText('LINESTRING(1 1, 2 0)', 3857))";
        assert_wkb_output(&ctx, sql, 3857).await;
    }
}
