use std::sync::{Arc, LazyLock};

use arrow_array::{Array, BinaryArray, BooleanArray, Int32Array};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::twkb::{self, Options};
use crate::util::args::{optional_bool_arg, optional_int_arg};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AsTWKB(geometry geom, integer prec=0, integer prec_z=0, integer prec_m=0,
/// boolean with_sizes=false, boolean with_boxes=false). The array form isn't planned.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer, Arg::Integer],
    &[
        Arg::Geometry,
        Arg::Integer,
        Arg::Integer,
        Arg::Integer,
        Arg::Boolean,
    ],
    &[
        Arg::Geometry,
        Arg::Integer,
        Arg::Integer,
        Arg::Integer,
        Arg::Boolean,
        Arg::Boolean,
    ],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec![
            "geom",
            "prec",
            "prec_z",
            "prec_m",
            "with_sizes",
            "with_boxes",
        ])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the geometry as TWKB, aka "Tiny Well-Known Binary"
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the geometry as Tiny Well-Known Binary (TWKB): coordinates rounded half away from zero to prec decimals (-7 to 7) for X and Y, prec_z and prec_m (0 to 7) for Z and M, and written as differences to the previous point. with_sizes adds each geometry's size in bytes, and with_boxes its bounding box. As in PostGIS, a line or polygon ring leaves out a point that rounds to the previous one, while two points are left in a line and four in a ring, and a multipoint leaves out its empty points. A NULL option means its default. The form for arrays of geometries and IDs isn't supported.",
    syntax_example = "ST_AsTWKB(geom, prec, prec_z, prec_m, with_sizes, with_boxes)",
    argument(name = "geom", description = "geometry"),
    argument(name = "prec", description = "integer, default 0"),
    argument(name = "prec_z", description = "integer, default 0"),
    argument(name = "prec_m", description = "integer, default 0"),
    argument(name = "with_sizes", description = "boolean, default false"),
    argument(name = "with_boxes", description = "boolean, default false"),
    related_udf(name = "st_geomfromtwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsTWKB;

impl AsTWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsTWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsTWKB {
    fn name(&self) -> &str {
        "st_astwkb"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Binary)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_twkb_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_twkb_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = AsTWKBKernel {
        precision_xy: optional_int_arg(&args, 1, 0)?,
        precision_z: optional_int_arg(&args, 2, 0)?,
        precision_m: optional_int_arg(&args, 3, 0)?,
        sizes: optional_bool_arg(&args, 4, false)?,
        boxes: optional_bool_arg(&args, 5, false)?,
    };
    let result: BinaryArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsTWKBKernel {
    precision_xy: Int32Array,
    precision_z: Int32Array,
    precision_m: Int32Array,
    sizes: BooleanArray,
    boxes: BooleanArray,
}

/// The row's value, or the default for NULL: PostGIS's function isn't strict.
fn int_or_zero(array: &Int32Array, row: usize) -> i32 {
    if array.is_null(row) {
        0
    } else {
        array.value(row)
    }
}

fn bool_or_false(array: &BooleanArray, row: usize) -> bool {
    !array.is_null(row) && array.value(row)
}

impl GeometryKernel for AsTWKBKernel {
    type Output = Vec<u8>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Vec<u8>>> {
        let options = Options {
            precision_xy: int_or_zero(&self.precision_xy, row),
            precision_z: int_or_zero(&self.precision_z, row),
            precision_m: int_or_zero(&self.precision_m, row),
            sizes: bool_or_false(&self.sizes, row),
            boxes: bool_or_false(&self.boxes, row),
        };
        let twkb = twkb::write(geom, &options)
            .map_err(|message| exec_datafusion_err!("st_astwkb: {message}"))?;
        Ok(Some(twkb))
    }
}
