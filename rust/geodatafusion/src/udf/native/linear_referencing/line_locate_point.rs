use std::sync::{Arc, LazyLock};

use arrow_array::Float64Array;
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, LineStringTrait};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_LineLocatePoint(geometry a_linestring, geometry a_point). The geography form
/// waits for the geography type.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["a_linestring", "a_point"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the fractional location of the closest point on a line to a point.
#[user_doc(
    doc_section(label = "Linear Referencing"),
    description = "Returns where on a LINESTRING the point closest to a_point lies, as a fraction (0 to 1) of the line's 2D length; the first of equally close segments wins. As in PostGIS, an empty line or point gives 0 and a line of no length 1. The geography form isn't supported yet.",
    syntax_example = "ST_LineLocatePoint(a_linestring, a_point)",
    argument(name = "a_linestring", description = "geometry"),
    argument(name = "a_point", description = "geometry"),
    related_udf(name = "st_lineinterpolatepoint"),
    related_udf(name = "st_linesubstring")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineLocatePoint;

impl LineLocatePoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LineLocatePoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineLocatePoint {
    fn name(&self) -> &str {
        "st_linelocatepoint"
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
        Ok(line_locate_point_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn line_locate_point_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = LineLocatePointKernel {
        point: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result: Float64Array = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct LineLocatePointKernel {
    point: OwnedColumn,
}

impl GeometryKernel for LineLocatePointKernel {
    type Output = f64;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        let Some(point) = self.point.get(row) else {
            return Ok(None);
        };
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!(
                "st_linelocatepoint: line_locate_point: 1st arg isn't a line"
            )
            .into());
        };
        let Wkt::Point(point) = point else {
            return Err(exec_datafusion_err!(
                "st_linelocatepoint: line_locate_point: 2nd arg isn't a point"
            )
            .into());
        };
        let coords: Vec<(f64, f64)> = line.coords().map(|c| (c.x(), c.y())).collect();
        let (Some(point), true) = (point.coord(), !coords.is_empty()) else {
            return Ok(Some(0.0));
        };
        let lengths: Vec<f64> = coords
            .windows(2)
            .map(|pair| (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1))
            .collect();
        let total: f64 = lengths.iter().sum();
        if total == 0.0 {
            return Ok(Some(1.0));
        }
        // The closest segment, the first of equally close ones, and where on it.
        let mut best: Option<(f64, usize, f64)> = None;
        for (index, pair) in coords.windows(2).enumerate() {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let length2 = dx * dx + dy * dy;
            let t = if length2 == 0.0 {
                0.0
            } else {
                (((point.x - ax) * dx + (point.y - ay) * dy) / length2).clamp(0.0, 1.0)
            };
            let distance = (point.x - (ax + dx * t)).hypot(point.y - (ay + dy * t));
            if best.is_none_or(|(best, _, _)| distance < best) {
                best = Some((distance, index, t));
            }
        }
        let Some((_, index, t)) = best else {
            return Ok(Some(0.0));
        };
        let before: f64 = lengths[..index].iter().sum();
        Ok(Some((before + lengths[index] * t) / total))
    }
}
