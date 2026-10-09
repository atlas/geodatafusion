use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, Dimensions, GeometryTrait, GeometryType, LineStringTrait};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::m;
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_InterpolatePoint(geometry line, geometry point).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["line", "point"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the interpolated measure of a geometry closest to a point.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns the M value of a LINESTRING with M at the point on it closest (in 2D) to the given POINT, interpolated along the segment; the first of equally close segments wins. Lines without M, empty inputs and other types are errors, as in PostGIS.",
    syntax_example = "ST_InterpolatePoint(line, point)",
    argument(name = "line", description = "geometry"),
    argument(name = "point", description = "geometry"),
    related_udf(name = "st_addmeasure"),
    related_udf(name = "st_locatealong")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct InterpolatePoint;

impl InterpolatePoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InterpolatePoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for InterpolatePoint {
    fn name(&self) -> &str {
        "st_interpolatepoint"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(interpolate_point_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn interpolate_point_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = InterpolatePointKernel {
        point: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct InterpolatePointKernel {
    point: OwnedColumn,
}

impl GeometryKernel for InterpolatePointKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(point) = self.point.get(row) else {
            return Ok(None);
        };
        if !matches!(geom.dim(), Dimensions::Xym | Dimensions::Xyzm) {
            return Err(exec_datafusion_err!(
                "st_interpolatepoint: ST_InterpolatePoint only accepts geometries that have an M dimension"
            )
            .into());
        }
        let Wkt::Point(point) = point else {
            return Err(
                exec_datafusion_err!("st_interpolatepoint: 2nd argument isn't a point").into(),
            );
        };
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(
                exec_datafusion_err!("st_interpolatepoint: 1st argument isn't a line").into(),
            );
        };
        let (Some(point), true) = (point.coord(), line.num_coords() > 0) else {
            return Err(
                exec_datafusion_err!("st_interpolatepoint: Input geometry is empty").into(),
            );
        };
        let coords: Vec<_> = line.coords().collect();
        let mut best: Option<(f64, f64)> = None;
        if let [only] = coords.as_slice() {
            return Ok(m(only));
        }
        for pair in coords.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (dx, dy) = (b.x() - a.x(), b.y() - a.y());
            let length2 = dx * dx + dy * dy;
            let t = if length2 == 0.0 {
                0.0
            } else {
                (((point.x - a.x()) * dx + (point.y - a.y()) * dy) / length2).clamp(0.0, 1.0)
            };
            let (cx, cy) = (a.x() + dx * t, a.y() + dy * t);
            let distance = (point.x - cx).hypot(point.y - cy);
            if best.is_none_or(|(best, _)| distance < best) {
                let (ma, mb) = (m(a).unwrap_or(0.0), m(b).unwrap_or(0.0));
                best = Some((distance, ma + (mb - ma) * t));
            }
        }
        Ok(best.map(|(_, measure)| measure))
    }
}
