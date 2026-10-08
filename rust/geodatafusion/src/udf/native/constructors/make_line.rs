//! ST_MakeLine, and its aggregate form.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::{
    Accumulator, AggregateUDFImpl, ColumnarValue, Documentation, GroupsAccumulator,
    ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString};

use crate::error::GeoDataFusionResult;
use crate::util::collect::{
    CollectAccumulator, CollectGroupsAccumulator, collect_groups_accumulator_supported,
    collect_state_fields,
};
use crate::util::field::{common_metadata, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args, single_geometry};

/// PostGIS: ST_MakeLine(geometry geom1, geometry geom2).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Makes a line from two points or lines.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a LineString from the points of two Point or LineString geometries, in order. A line's first point is left out if it repeats the last point so far; repeated points are otherwise kept. Empty geometries add no points. The result has Z or M if an input has; missing values are 0. Other geometry types are an error. For the aggregate form, see ST_MakeLine_Agg; the geometry[] form isn't supported.",
    syntax_example = "ST_MakeLine(geom1, geom2)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    related_udf(name = "st_makeline_agg")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeLine;

impl MakeLine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakeLine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakeLine {
    fn name(&self) -> &str {
        "st_makeline"
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
        Ok(make_line_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_line_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = MakeLineKernel {
        geom2: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct MakeLineKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for MakeLineKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // ST_MakeLine is STRICT: SQL NULL in any argument gives SQL NULL.
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let geoms = [to_owned_geometry(geom1), geom2.clone()];
        make_line(&geoms, Inputs::PointsAndLines)
    }
}

/// The aggregate form of ST_MakeLine.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Aggregate that creates a LineString from the points of a set of Point, MultiPoint and LineString geometries, in input order or the call's ORDER BY. A line's first point is left out if it repeats the last point so far. Other geometry types and NULLs are skipped; NULL is returned if nothing is left. The result has Z or M if an input has; missing values are 0. This is PostGIS's aggregate ST_MakeLine(geometry), named apart from the scalar ST_MakeLine.",
    syntax_example = "ST_MakeLine_Agg(geom ORDER BY expression)",
    argument(name = "geom", description = "geometry"),
    related_udf(name = "st_makeline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakeLineAgg;

impl MakeLineAgg {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakeLineAgg {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for MakeLineAgg {
    fn name(&self) -> &str {
        "st_makeline_agg"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field should be called instead")
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&arg_fields[0]),
        ))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        Ok(Box::new(CollectAccumulator::try_new(args, make_line_agg)?))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        collect_state_fields(args)
    }

    fn groups_accumulator_supported(&self, args: AccumulatorArgs) -> bool {
        collect_groups_accumulator_supported(args)
    }

    fn create_groups_accumulator(
        &self,
        args: AccumulatorArgs,
    ) -> Result<Box<dyn GroupsAccumulator>> {
        Ok(Box::new(CollectGroupsAccumulator::try_new(
            args,
            make_line_agg,
        )?))
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_line_agg(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    make_line(&geoms, Inputs::AnySkipped)
}

/// Which inputs a form of ST_MakeLine takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Inputs {
    /// Points and lines; anything else is an error.
    PointsAndLines,
    /// Points, multipoints and lines; anything else is skipped.
    AnySkipped,
}

/// The line through the points of `geoms`, or `None` if none of them is a point or a line.
fn make_line(geoms: &[Wkt<f64>], inputs: Inputs) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    let mut coords: Vec<Coord<f64>> = Vec::new();
    let mut usable = false;
    for geom in geoms {
        match geom {
            Wkt::Point(point) => {
                usable = true;
                coords.extend(point.coord().cloned());
            }
            Wkt::LineString(line) => {
                usable = true;
                // The point where two lines meet is kept once.
                let skip = match (coords.last(), line.coords().first()) {
                    (Some(last), Some(first)) => usize::from(last == first),
                    _ => 0,
                };
                coords.extend(line.coords().iter().skip(skip).cloned());
            }
            Wkt::MultiPoint(points) if inputs == Inputs::AnySkipped => {
                usable = true;
                coords.extend(
                    points
                        .points()
                        .iter()
                        .filter_map(|point| point.coord().cloned()),
                );
            }
            _ if inputs == Inputs::AnySkipped => {}
            _ => {
                return Err(
                    exec_datafusion_err!("Input geometries must be points or lines").into(),
                );
            }
        }
    }
    if !usable {
        return Ok(None);
    }
    // The line has Z or M if any input has; the others' points get 0.
    let has_z = geoms.iter().any(|geom| has_z(geom.dimension()));
    let has_m = geoms.iter().any(|geom| has_m(geom.dimension()));
    let dim = match (has_z, has_m) {
        (false, false) => Dimension::XY,
        (true, false) => Dimension::XYZ,
        (false, true) => Dimension::XYM,
        (true, true) => Dimension::XYZM,
    };
    for coord in &mut coords {
        coord.z = has_z.then(|| coord.z.unwrap_or(0.0));
        coord.m = has_m.then(|| coord.m.unwrap_or(0.0));
    }
    Ok(Some(Wkt::LineString(LineString::new(coords, dim))))
}

fn has_z(dim: Dimension) -> bool {
    matches!(dim, Dimension::XYZ | Dimension::XYZM)
}

fn has_m(dim: Dimension) -> bool {
    matches!(dim, Dimension::XYM | Dimension::XYZM)
}
