use std::sync::Arc;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType, PolygonTrait};
use wkt::Wkt;
use wkt::types::Polygon;

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::util::geometry_dump::{
    GeometryDump, geometry_dump_array, geometry_dump_field,
};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::single_geometry;

/// Returns a set of geometry_dump rows for the exterior and interior rings of a Polygon.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "A set-returning function that extracts the rings of a polygon as polygons, with Z and M: the shell with path {0}, then the holes with paths {1}, {2} and so on. Any other geometry type is an error. Returns a list of (path, geom) records per row: unnest it for rows, or call st_dumprings(geom) in FROM with a constant argument.",
    syntax_example = "ST_DumpRings(geom)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_dump"),
    related_udf(name = "st_exteriorring"),
    related_udf(name = "st_interiorringn")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct DumpRings;

impl DumpRings {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DumpRings {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for DumpRings {
    fn name(&self) -> &str {
        "st_dumprings"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_dump_field(self.name(), &args.arg_fields[0]))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dump_rings_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dump_rings_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let rows: Vec<Option<GeometryDump>> = map_geometry(geometries.as_ref(), &DumpRingsKernel)?;
    let result = geometry_dump_array(rows, &args.return_field)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DumpRingsKernel;

impl GeometryKernel for DumpRingsKernel {
    type Output = GeometryDump;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<GeometryDump>> {
        let GeometryType::Polygon(polygon) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_dumprings: Input is not a polygon").into());
        };
        let dim = dimension(geom.dim());
        // The shell's path is 0 and the holes' are 1 and up.
        let rings = polygon.exterior().into_iter().chain(polygon.interiors());
        let dump = (0..)
            .zip(rings)
            .map(|(index, ring)| {
                let ring = Polygon::new(vec![owned_line_string(&ring, dim)], dim);
                (vec![index], Wkt::Polygon(ring))
            })
            .collect();
        Ok(Some(dump))
    }
}
