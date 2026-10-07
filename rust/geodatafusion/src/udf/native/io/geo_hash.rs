//! ST_GeoHash.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundsKernel;
use crate::udf::native::io::util::geohash::encode;
use crate::util::args::optional_int_arg;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_GeoHash(geometry geom, integer maxchars = 0).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "maxchars"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the GeoHash of a geometry's bounding box.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Computes a GeoHash representation of a geometry. A GeoHash encodes a geographic Point into a text form that is sortable and searchable based on prefixing. A shorter GeoHash is a less precise representation of a point. It can be thought of as a box that contains the point. Non-point geometries with non-zero extent can also be mapped to GeoHash codes. The precision of the code depends on the geographic extent of the geometry. If maxchars is not specified or is 0 or less, the returned GeoHash code is for the smallest cell containing the input geometry; points return a GeoHash with 20 characters. Otherwise the GeoHash of the geometry's bounding box centre has maxchars characters. The geometry must be in geographic (longitude/latitude) coordinates. Empty geometries return NULL.",
    syntax_example = "ST_GeoHash(geom, maxchars)",
    argument(name = "geom", description = "geometry"),
    argument(name = "maxchars", description = "integer")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeoHash;

impl GeoHash {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeoHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeoHash {
    fn name(&self) -> &str {
        "st_geohash"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geo_hash_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geo_hash_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = GeoHashKernel {
        max_chars: optional_int_arg(&args, 1, 0)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct GeoHashKernel {
    max_chars: Int32Array,
}

impl GeometryKernel for GeoHashKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // SQL NULL in, SQL NULL out.
        if self.max_chars.is_null(row) {
            return Ok(None);
        }
        // PostGIS returns NULL for EMPTY.
        let Some(bounds) = (BoundsKernel { include_z: false }).eval(geom, row)? else {
            return Ok(None);
        };
        let [xmin, ymin, _, xmax, ymax, _] = bounds.raw_bounds();
        let mut hash = String::new();
        encode(
            &mut hash,
            &[xmin, ymin, xmax, ymax],
            self.max_chars.value(row),
        )
        .map_err(|e| exec_datafusion_err!("st_geohash: {e}"))?;
        Ok(Some(hash))
    }
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::constructors::Point;

    #[tokio::test]
    async fn test_geohash() {
        let ctx = SessionContext::new();
        ctx.register_udf(GeoHash.into());
        ctx.register_udf(Point.into());

        let df = ctx
            .sql("SELECT ST_GeoHash(ST_Point(-126, 48)), ST_GeoHash(ST_Point(-126, 48), 5)")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();

        assert_eq!(
            batch.column(0).as_string::<i32>().value(0),
            "c0w3hf1s70w3hf1s70w3"
        );
        assert_eq!(batch.column(1).as_string::<i32>().value(0), "c0w3h");
    }
}
