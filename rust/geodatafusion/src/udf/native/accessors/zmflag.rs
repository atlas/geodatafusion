//! ST_Zmflag, ST_HasZ and ST_HasM: a geometry's coordinate dimension.

use std::sync::Arc;

use arrow_array::{BooleanArray, Int16Array};
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{Dimensions, GeometryTrait};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::single_geometry;

/// Returns a code indicating the ZM coordinate dimension of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns a code for the coordinate dimension of a geometry: 0 for 2D, 1 for 3DM, 2 for 3DZ and 3 for 4D. An empty geometry has the dimension it was written with.",
    syntax_example = "ST_Zmflag(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_hasz"),
    related_udf(name = "st_hasm"),
    related_udf(name = "st_ndims")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Zmflag;

impl Zmflag {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Zmflag {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Zmflag {
    fn name(&self) -> &str {
        "st_zmflag"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int16)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(zmflag_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Checks if a geometry has a Z dimension.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if the geometry has Z coordinates, including an empty geometry written with Z.",
    syntax_example = "ST_HasZ(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_hasm"),
    related_udf(name = "st_zmflag")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct HasZ;

impl HasZ {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HasZ {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for HasZ {
    fn name(&self) -> &str {
        "st_hasz"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(has_impl(args, Ordinate::Z)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Checks if a geometry has an M (measure) dimension.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns true if the geometry has M coordinates, including an empty geometry written with M.",
    syntax_example = "ST_HasM(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_hasz"),
    related_udf(name = "st_zmflag")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct HasM;

impl HasM {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HasM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for HasM {
    fn name(&self) -> &str {
        "st_hasm"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(has_impl(args, Ordinate::M)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn zmflag_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Int16Array = map_geometry(geometries.as_ref(), &ZmflagKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

fn has_impl(args: ScalarFunctionArgs, ordinate: Ordinate) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: BooleanArray = map_geometry(geometries.as_ref(), &ordinate)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

/// PostGIS's ZM flag: bit 0 is M, bit 1 is Z. Read per row, because a column in a
/// mixed-dimension encoding can hold every dimension.
fn zmflag(geom: &impl GeometryTrait<T = f64>) -> i16 {
    match geom.dim() {
        Dimensions::Xy | Dimensions::Unknown(_) => 0,
        Dimensions::Xym => 1,
        Dimensions::Xyz => 2,
        Dimensions::Xyzm => 3,
    }
}

struct ZmflagKernel;

impl GeometryKernel for ZmflagKernel {
    type Output = i16;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<i16>> {
        Ok(Some(zmflag(geom)))
    }
}

#[derive(Debug, Clone, Copy)]
enum Ordinate {
    Z,
    M,
}

impl GeometryKernel for Ordinate {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        let flag = zmflag(geom);
        Ok(Some(match self {
            Ordinate::Z => flag & 2 != 0,
            Ordinate::M => flag & 1 != 0,
        }))
    }
}
