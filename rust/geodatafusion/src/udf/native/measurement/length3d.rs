//! ST_3DLength and ST_3DPerimeter: lengths measured in 3D.

use std::sync::Arc;

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPolygonTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::z;
use crate::util::owned::to_owned_geometry;
use crate::util::signature::single_geometry;

/// Returns the 3D length of a linear geometry.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the length of the lines of a geometry, measured in 3D when it has Z and in 2D otherwise, including the lines of a GEOMETRYCOLLECTION. Polygons and points have length 0; use ST_3DPerimeter for polygons.",
    syntax_example = "ST_3DLength(a_3dlinestring)",
    argument(name = "a_3dlinestring", description = "geometry"),
    related_udf(name = "st_length"),
    related_udf(name = "st_3dperimeter")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Length3D;

impl Length3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Length3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Length3D {
    fn name(&self) -> &str {
        "st_3dlength"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(measure_impl(args, Measure::Length)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the 3D perimeter of a polygonal geometry.
#[user_doc(
    doc_section(label = "Measurement Functions"),
    description = "Returns the length of the polygon rings of a geometry, measured in 3D when it has Z and in 2D otherwise, interior rings and the polygons of a GEOMETRYCOLLECTION included. Lines and points have perimeter 0.",
    syntax_example = "ST_3DPerimeter(geomA)",
    argument(name = "geomA", description = "geometry"),
    related_udf(name = "st_perimeter"),
    related_udf(name = "st_3dlength")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Perimeter3D;

impl Perimeter3D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Perimeter3D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Perimeter3D {
    fn name(&self) -> &str {
        "st_3dperimeter"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(measure_impl(args, Measure::Perimeter)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn measure_impl(args: ScalarFunctionArgs, measure: Measure) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result: Float64Array = map_geometry(geometries.as_ref(), &measure)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

#[derive(Debug, Clone, Copy)]
enum Measure {
    /// The lines.
    Length,
    /// The polygon rings.
    Perimeter,
}

impl GeometryKernel for Measure {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(Some(self.measure(geom)))
    }
}

impl Measure {
    fn measure(self, geom: &impl GeometryTrait<T = f64>) -> f64 {
        match (self, geom.as_type()) {
            (Measure::Length, GeometryType::LineString(line)) => line_length(line),
            (Measure::Length, GeometryType::MultiLineString(lines)) => {
                lines.line_strings().map(|line| line_length(&line)).sum()
            }
            (Measure::Perimeter, GeometryType::Polygon(polygon)) => polygon_perimeter(polygon),
            (Measure::Perimeter, GeometryType::MultiPolygon(polygons)) => polygons
                .polygons()
                .map(|polygon| polygon_perimeter(&polygon))
                .sum(),
            (_, GeometryType::GeometryCollection(collection)) => collection
                .geometries()
                .map(|member| self.measure(&member))
                .sum(),
            // As the line or polygon PostGIS would see.
            (_, GeometryType::Line(_) | GeometryType::Rect(_) | GeometryType::Triangle(_)) => {
                self.measure(&to_owned_geometry(geom))
            }
            _ => 0.0,
        }
    }
}

fn polygon_perimeter(polygon: &impl PolygonTrait<T = f64>) -> f64 {
    polygon
        .exterior()
        .into_iter()
        .chain(polygon.interiors())
        .map(|ring| line_length(&ring))
        .sum()
}

fn line_length(line: &impl LineStringTrait<T = f64>) -> f64 {
    let coords: Vec<_> = line.coords().collect();
    coords
        .windows(2)
        .map(|pair| {
            let (a, b) = (&pair[0], &pair[1]);
            let (dx, dy) = (b.x() - a.x(), b.y() - a.y());
            let dz = z(a).zip(z(b)).map_or(0.0, |(za, zb)| zb - za);
            (dx * dx + dy * dy + dz * dz).sqrt()
        })
        .sum()
}
