//! ST_MakeValid.

use std::sync::LazyLock;

use arrow_array::{Array, StringArray};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use geoarrow_array::GeoArrowArray;
use geos::{Geom, MakeValidMethod, MakeValidParams};
use wkt::Wkt;
use wkt::types::{MultiLineString, MultiPoint, MultiPolygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::util::args::optional_text_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_MakeValid(geometry input) and ST_MakeValid(geometry input, text params).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Text]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["input", "params"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Makes an invalid geometry valid.
#[user_doc(
    doc_section(label = "Geometry Validation"),
    description = "Creates a valid representation of an invalid geometry without losing any of the input vertices; valid geometries are returned as they are. The params string holds space-separated key=value pairs: method=linework (the default) or method=structure, and, for the structure method, keepcollapsed=true or false (the default), which keeps components that collapse to a lower dimension. Unknown keys are ignored. This function keeps Z and drops M.",
    syntax_example = "ST_MakeValid(input, params)",
    argument(name = "input", description = "geometry"),
    argument(name = "params", description = "text")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeValid;

impl MakeValid {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakeValid {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakeValid {
    fn name(&self) -> &str {
        "st_makevalid"
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
        Ok(make_valid_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_valid_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = MakeValidKernel {
        params: optional_text_arg(&args, 1, "")?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct MakeValidKernel {
    params: StringArray,
}

impl GeometryKernel for MakeValidKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_MakeValid is STRICT: SQL NULL in any argument gives SQL NULL.
        if self.params.is_null(row) {
            return Ok(None);
        }
        // Unlike most GEOS-backed PostGIS functions, ST_MakeValid doesn't return EMPTY input
        // unchanged: an EMPTY input loses M too.
        let params = parse_params(self.params.value(row))?;
        let valid = from_geos(&to_geos(geom)?.make_valid_with_params(&params)?)?;
        // PostGIS keeps a collection a collection: a single result becomes a multi.
        let is_collection = matches!(
            geom.as_type(),
            GeometryType::MultiPoint(_)
                | GeometryType::MultiLineString(_)
                | GeometryType::MultiPolygon(_)
                | GeometryType::GeometryCollection(_)
        );
        Ok(Some(if is_collection {
            as_multi(valid)
        } else {
            valid
        }))
    }
}

/// A single geometry as the multi-geometry holding it; collections are returned as they are.
fn as_multi(geom: Wkt<f64>) -> Wkt<f64> {
    match geom {
        Wkt::Point(point) => {
            let dim = point.dimension();
            Wkt::MultiPoint(MultiPoint::new(vec![point], dim))
        }
        Wkt::LineString(line) => {
            let dim = line.dimension();
            Wkt::MultiLineString(MultiLineString::new(vec![line], dim))
        }
        Wkt::Polygon(polygon) => {
            let dim = polygon.dimension();
            Wkt::MultiPolygon(MultiPolygon::new(vec![polygon], dim))
        }
        collection => collection,
    }
}

/// The GEOS parameters of a PostGIS `params` string: space-separated `key=value` pairs, with
/// PostGIS's defaults, values and errors. Unknown keys are ignored, as in PostGIS.
fn parse_params(params: &str) -> GeoDataFusionResult<MakeValidParams> {
    let mut method = MakeValidMethod::Linework;
    let mut keep_collapsed = false;
    for (key, value) in params
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
    {
        match key {
            "method" if value.eq_ignore_ascii_case("linework") => {
                method = MakeValidMethod::Linework
            }
            "method" if value.eq_ignore_ascii_case("structure") => {
                method = MakeValidMethod::Structure
            }
            "method" => {
                return Err(exec_datafusion_err!(
                    "st_makevalid: Unsupported value for 'method', '{value}'. Use 'linework' or \
                     'structure'"
                )
                .into());
            }
            "keepcollapsed" if value.eq_ignore_ascii_case("true") => keep_collapsed = true,
            "keepcollapsed" if value.eq_ignore_ascii_case("false") => keep_collapsed = false,
            "keepcollapsed" => {
                return Err(exec_datafusion_err!(
                    "st_makevalid: Unsupported value for 'keepcollapsed', '{value}'. Use 'true' \
                     or 'false'"
                )
                .into());
            }
            _ => {}
        }
    }
    Ok(MakeValidParams::builder()
        .method(method)
        .keep_collapsed(keep_collapsed)
        .build()?)
}
