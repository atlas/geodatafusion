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
use geo_traits::{Dimensions, GeometryTrait};
use wkt::Wkt;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_text_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::map_coords;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_SwapOrdinates(geometry geom, cstring ords).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Text]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "ords"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a version of the given geometry with given ordinate values swapped.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the geometry with two ordinates swapped. ords names them with two letters from x, y, z and m, in either case ('xz', 'MY'). Naming an ordinate the geometry doesn't have is an error, even for an empty geometry.",
    syntax_example = "ST_SwapOrdinates(geom, ords)",
    argument(name = "geom", description = "geometry"),
    argument(name = "ords", description = "text, two of x, y, z and m"),
    related_udf(name = "st_flipcoordinates")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SwapOrdinates;

impl SwapOrdinates {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SwapOrdinates {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SwapOrdinates {
    fn name(&self) -> &str {
        "st_swapordinates"
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
        Ok(swap_ordinates_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn swap_ordinates_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = SwapOrdinatesKernel {
        ords: optional_text_arg(&args, 1, "")?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct SwapOrdinatesKernel {
    ords: StringArray,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ordinate {
    X,
    Y,
    Z,
    M,
}

impl Ordinate {
    fn get(self, coord: &Coord<f64>) -> f64 {
        match self {
            Ordinate::X => coord.x,
            Ordinate::Y => coord.y,
            // The geometry has the ordinate; that was checked first.
            Ordinate::Z => coord.z.unwrap_or_default(),
            Ordinate::M => coord.m.unwrap_or_default(),
        }
    }

    fn set(self, coord: &mut Coord<f64>, value: f64) {
        match self {
            Ordinate::X => coord.x = value,
            Ordinate::Y => coord.y = value,
            Ordinate::Z => coord.z = Some(value),
            Ordinate::M => coord.m = Some(value),
        }
    }
}

/// The two ordinates `ords` names, with PostGIS's error messages.
fn parse_ords(ords: &str) -> GeoDataFusionResult<(Ordinate, Ordinate)> {
    let letters: Vec<char> = ords.chars().collect();
    let [first, second] = letters[..] else {
        return Err(exec_datafusion_err!(
            "st_swapordinates: Invalid ordinate specification. Need two letters from the set (x,y,z,m). Got '{ords}'"
        )
        .into());
    };
    let ordinate = |letter: char| match letter.to_ascii_lowercase() {
        'x' => Ok(Ordinate::X),
        'y' => Ok(Ordinate::Y),
        'z' => Ok(Ordinate::Z),
        'm' => Ok(Ordinate::M),
        _ => Err(exec_datafusion_err!(
            "st_swapordinates: Invalid ordinate name '{letter}'. Expected x,y,z or m"
        )),
    };
    Ok((ordinate(first)?, ordinate(second)?))
}

impl GeometryKernel for SwapOrdinatesKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.ords.is_null(row) {
            return Ok(None);
        }
        let (first, second) = parse_ords(self.ords.value(row))?;
        let (has_z, has_m) = match geom.dim() {
            Dimensions::Xyz => (true, false),
            Dimensions::Xym => (false, true),
            Dimensions::Xyzm => (true, true),
            Dimensions::Xy | Dimensions::Unknown(_) => (false, false),
        };
        for ordinate in [first, second] {
            if ordinate == Ordinate::Z && !has_z {
                return Err(exec_datafusion_err!(
                    "st_swapordinates: Geometry does not have a Z ordinate"
                )
                .into());
            }
            if ordinate == Ordinate::M && !has_m {
                return Err(exec_datafusion_err!(
                    "st_swapordinates: Geometry does not have an M ordinate"
                )
                .into());
            }
        }
        Ok(Some(map_coords(geom, &|mut c| {
            let (a, b) = (first.get(&c), second.get(&c));
            first.set(&mut c, b);
            second.set(&mut c, a);
            c
        })))
    }
}
