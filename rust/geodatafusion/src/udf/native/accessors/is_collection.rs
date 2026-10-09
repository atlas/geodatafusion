use std::sync::Arc;

use arrow_array::BooleanArray;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Tests if a geometry is a geometry collection type.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if the geometry is a collection type: MULTIPOINT, MULTILINESTRING, MULTIPOLYGON or GEOMETRYCOLLECTION, even if it is empty or has one member.",
    syntax_example = "ST_IsCollection(g)",
    argument(name = "g", description = "geometry"),
    related_udf(name = "st_numgeometries")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct IsCollection;

impl IsCollection {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IsCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for IsCollection {
    fn name(&self) -> &str {
        "st_iscollection"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(is_collection_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn is_collection_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &IsCollectionKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct IsCollectionKernel;

impl GeometryKernel for IsCollectionKernel {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        Ok(Some(matches!(
            geom.as_type(),
            GeometryType::MultiPoint(_)
                | GeometryType::MultiLineString(_)
                | GeometryType::MultiPolygon(_)
                | GeometryType::GeometryCollection(_)
        )))
    }
}
