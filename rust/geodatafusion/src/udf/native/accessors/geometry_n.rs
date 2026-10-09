use std::sync::LazyLock;

use arrow_array::{Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, GeometryType, MultiLineStringTrait, MultiPointTrait,
    MultiPolygonTrait,
};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{
    line_string_to_owned, point_to_owned, polygon_to_owned, to_owned_geometry,
};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_GeometryN(geometry geomA, integer n).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geomA", "n"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Return an element of a geometry collection.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the 1-based nth member of a collection (MULTI* or GEOMETRYCOLLECTION), and the geometry itself for n = 1 if it isn't a collection. Returns NULL if n is out of range or the geometry is empty, including a collection of empty members.",
    syntax_example = "ST_GeometryN(geomA, n)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "n", description = "integer"),
    related_udf(name = "st_numgeometries")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeometryN;

impl GeometryN {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeometryN {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeometryN {
    fn name(&self) -> &str {
        "st_geometryn"
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
        Ok(geometry_n_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geometry_n_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = GeometryNKernel {
        n: optional_int_arg(&args, 1, 1)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct GeometryNKernel {
    n: Int32Array,
}

impl GeometryKernel for GeometryNKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.n.is_null(row) || is_geometry_topologically_empty(geom) {
            return Ok(None);
        }
        // 1-based; zero and negative numbers are out of range.
        let Some(index) = usize::try_from(self.n.value(row))
            .ok()
            .and_then(|n| n.checked_sub(1))
        else {
            return Ok(None);
        };
        let dim = geom.dim();
        Ok(match geom.as_type() {
            GeometryType::MultiPoint(points) => {
                points.point(index).map(|m| point_to_owned(&m, dim))
            }
            GeometryType::MultiLineString(lines) => lines
                .line_string(index)
                .map(|m| line_string_to_owned(&m, dim)),
            GeometryType::MultiPolygon(polygons) => {
                polygons.polygon(index).map(|m| polygon_to_owned(&m, dim))
            }
            GeometryType::GeometryCollection(collection) => {
                collection.geometry(index).map(|m| to_owned_geometry(&m))
            }
            _ => (index == 0).then(|| to_owned_geometry(geom)),
        })
    }
}
