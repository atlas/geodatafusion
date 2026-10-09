use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryCollectionTrait, GeometryTrait, GeometryType, MultiPolygonTrait};

use crate::error::GeoDataFusionResult;
use crate::udf::native::measurement::length3d::polygon_perimeter;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::single_geometry;

static ALIASES: LazyLock<Vec<String>> = LazyLock::new(|| vec!["st_perimeter2d".to_string()]);

/// Returns the length of the boundary of a polygonal geometry or geography.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the 2D length of the polygon rings of a geometry, interior rings and the polygons of a GEOMETRYCOLLECTION included. Lines and points have perimeter 0; Z and M are ignored. ST_Perimeter2D is an alias. The geography form isn't supported yet.",
    syntax_example = "ST_Perimeter(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_length"),
    related_udf(name = "st_3dperimeter")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Perimeter;

impl Perimeter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Perimeter {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Perimeter {
    fn name(&self) -> &str {
        "st_perimeter"
    }

    fn aliases(&self) -> &[String] {
        &ALIASES
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(perimeter_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn perimeter_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &PerimeterKernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct PerimeterKernel;

impl GeometryKernel for PerimeterKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(perimeter(geom)))
    }
}

fn perimeter(geom: &impl GeometryTrait<T = f64>) -> f64 {
    match geom.as_type() {
        GeometryType::Polygon(polygon) => polygon_perimeter(polygon, false),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .map(|polygon| polygon_perimeter(&polygon, false))
            .sum(),
        GeometryType::GeometryCollection(collection) => collection
            .geometries()
            .map(|member| perimeter(&member))
            .sum(),
        // As the polygon PostGIS would see.
        GeometryType::Rect(_) | GeometryType::Triangle(_) => perimeter(&to_owned_geometry(geom)),
        _ => 0.0,
    }
}
