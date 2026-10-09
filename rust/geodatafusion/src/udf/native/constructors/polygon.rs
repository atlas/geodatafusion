use std::sync::{Arc, LazyLock};

use arrow_array::new_null_array;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use geoarrow_schema::Metadata;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::native::constructors::make_polygon::polygon_from_rings;
use crate::util::args::scalar_srid;
use crate::util::field::{geometry_array, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_Polygon(geometry lineString, integer srid). PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Srid]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Creates a Polygon from a LineString with a specified SRID.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns a POLYGON with the given LINESTRING as its shell and the given SRID. The ring must be closed (in 2D) and have at least 4 points. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_Polygon(lineString, srid)",
    argument(name = "lineString", description = "geometry, a LINESTRING"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_makepolygon")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Polygon;

impl Polygon {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Polygon {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Polygon {
    fn name(&self) -> &str {
        "st_polygon"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let crs = scalar_srid(self.name(), &args, 1)?
            .map(srid_to_crs)
            .unwrap_or_default();
        Ok(wkb_return_field(
            self.name(),
            Arc::new(Metadata::new(crs, None)),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(polygon_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn polygon_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    // A NULL SRID gives NULL in every row.
    if matches!(args.args.get(1), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &PolygonKernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PolygonKernel;

impl GeometryKernel for PolygonKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let GeometryType::LineString(shell) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_polygon: Shell is not a line").into());
        };
        let dim = dimension(geom.dim());
        let polygon = polygon_from_rings("st_polygon", vec![owned_line_string(shell, dim)], dim)?;
        Ok(Some(Wkt::Polygon(polygon)))
    }
}
