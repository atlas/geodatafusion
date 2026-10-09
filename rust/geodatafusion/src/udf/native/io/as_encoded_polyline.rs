use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, GeometryTrait, GeometryType, LineStringTrait, MultiPointTrait, PointTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::polyline::{DEFAULT_PRECISION, encode};
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::crs_to_srid;

/// PostGIS: ST_AsEncodedPolyline(geometry geom, integer nprecision = 5).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "nprecision"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns an Encoded Polyline from a LineString geometry.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns a LINESTRING or MULTIPOINT as an Encoded Polyline, with nprecision decimal digits (default 5; a negative value means the default). The geometry must have SRID 4326. Z and M are dropped, and an empty LINESTRING gives an empty string. As in PostGIS, precisions above 7 overflow 32-bit integers and give garbage.",
    syntax_example = "ST_AsEncodedPolyline(geom, nprecision)",
    argument(name = "geom", description = "geometry"),
    argument(name = "nprecision", description = "integer, default 5"),
    related_udf(name = "st_linefromencodedpolyline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsEncodedPolyline;

impl AsEncodedPolyline {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsEncodedPolyline {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsEncodedPolyline {
    fn name(&self) -> &str {
        "st_asencodedpolyline"
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
        Ok(as_encoded_polyline_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_encoded_polyline_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsEncodedPolylineKernel {
        srid: crs_to_srid(input_metadata(&args.arg_fields[0]).crs()),
        precision: optional_int_arg(&args, 1, DEFAULT_PRECISION)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsEncodedPolylineKernel {
    /// The column's SRID.
    srid: Option<i32>,
    precision: Int32Array,
}

impl GeometryKernel for AsEncodedPolylineKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        if self.precision.is_null(row) {
            return Ok(None);
        }
        if self.srid != Some(4326) {
            return Err(
                exec_datafusion_err!("st_asencodedpolyline: Only SRID 4326 is supported.").into(),
            );
        }
        let precision = self.precision.value(row);
        let positions: Vec<(f64, f64)> = match geom.as_type() {
            GeometryType::LineString(line) => line.coords().map(|c| (c.x(), c.y())).collect(),
            GeometryType::MultiPoint(points) => points
                .points()
                .map(|point| {
                    point.coord().map(|c| (c.x(), c.y())).ok_or_else(|| {
                        exec_datafusion_err!(
                            "st_asencodedpolyline: an empty point can't be encoded"
                        )
                    })
                })
                .collect::<Result<_>>()?,
            other => {
                let type_name = match other {
                    GeometryType::Point(_) => "Point",
                    GeometryType::Polygon(_)
                    | GeometryType::Rect(_)
                    | GeometryType::Triangle(_) => "Polygon",
                    GeometryType::MultiLineString(_) => "MultiLineString",
                    GeometryType::MultiPolygon(_) => "MultiPolygon",
                    _ => "GeometryCollection",
                };
                return Err(exec_datafusion_err!(
                    "st_asencodedpolyline: '{type_name}' geometry type not supported"
                )
                .into());
            }
        };
        Ok(Some(encode(positions, precision)))
    }
}
