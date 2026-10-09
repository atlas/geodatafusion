use std::sync::LazyLock;

use arrow_array::{Array, BooleanArray};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{Dimensions, GeometryTrait};
use wkt::types::{Coord, LineString};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::util::float_box::FloatBox;
use crate::util::args::optional_bool_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::dimension;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_BoundingDiagonal(geometry geom, boolean fits = false).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Boolean]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "fits"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the diagonal of a geometry's bounding box.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "Returns the diagonal of a geometry's bounding box as a LINESTRING from its minimum to its maximum corner, with Z and M if the geometry has them. By default (fits = false) the box is the one PostGIS stores in the geometry, rounded outward to single precision; with fits = true it is exact. An empty geometry gives LINESTRING EMPTY, or a diagonal of zeros with fits = true.",
    syntax_example = "ST_BoundingDiagonal(geom, fits)",
    argument(name = "geom", description = "geometry"),
    argument(name = "fits", description = "boolean, default false"),
    related_udf(name = "st_envelope")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct BoundingDiagonal;

impl BoundingDiagonal {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BoundingDiagonal {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for BoundingDiagonal {
    fn name(&self) -> &str {
        "st_boundingdiagonal"
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
        Ok(bounding_diagonal_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn bounding_diagonal_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = BoundingDiagonalKernel {
        fits: optional_bool_arg(&args, 1, false)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct BoundingDiagonalKernel {
    fits: BooleanArray,
}

impl GeometryKernel for BoundingDiagonalKernel {
    type Output = LineString<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<LineString<f64>>> {
        if self.fits.is_null(row) {
            return Ok(None);
        }
        let dim = geom.dim();
        let (has_z, has_m) = match dim {
            Dimensions::Xyz => (true, false),
            Dimensions::Xym => (false, true),
            Dimensions::Xyzm => (true, true),
            Dimensions::Xy | Dimensions::Unknown(_) => (false, false),
        };
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        let (x, y, z, m) = if self.fits.value(row) {
            if rect.is_empty() {
                // PostGIS computes the exact box of an empty geometry as all zeros.
                ((0.0, 0.0), (0.0, 0.0), Some((0.0, 0.0)), Some((0.0, 0.0)))
            } else {
                (
                    (rect.minx(), rect.maxx()),
                    (rect.miny(), rect.maxy()),
                    rect.z_range(),
                    rect.m_range(),
                )
            }
        } else {
            let Some(float_box) = FloatBox::new(&rect) else {
                return Ok(Some(LineString::new(vec![], dimension(dim))));
            };
            (float_box.x, float_box.y, float_box.z, float_box.m)
        };
        let corner = |pick: fn((f64, f64)) -> f64| Coord {
            x: pick(x),
            y: pick(y),
            z: z.filter(|_| has_z).map(pick),
            m: m.filter(|_| has_m).map(pick),
        };
        let coords = vec![corner(|range| range.0), corner(|range| range.1)];
        Ok(Some(LineString::new(coords, dimension(dim))))
    }
}
