//! ST_AddPoint, ST_SetPoint and ST_RemovePoint: editing the points of a LINESTRING.

use std::sync::LazyLock;

use arrow_array::{Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::Wkt;
use wkt::types::{Coord, LineString};

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_AddPoint(geometry linestring, geometry point, integer position = -1).
static ADD_POINT_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Integer],
];

static ADD_POINT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["linestring", "point", "position"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS: ST_SetPoint(geometry linestring, integer zerobasedposition, geometry point).
static SET_POINT_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Integer, Arg::Geometry]];

static SET_POINT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["linestring", "zerobasedposition", "point"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS: ST_RemovePoint(geometry linestring, integer offset).
static REMOVE_POINT_ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Integer]];

static REMOVE_POINT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["linestring", "offset"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Add a point to a LineString.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the LINESTRING with the POINT inserted before the 0-based position, or appended when position is -1 (the default) or the number of points. The point takes the line's dimension (a missing Z or M is 0), and an empty point leaves the line unchanged. As in PostGIS, the SRIDs aren't compared.",
    syntax_example = "ST_AddPoint(linestring, point, position)",
    argument(name = "linestring", description = "geometry"),
    argument(name = "point", description = "geometry"),
    argument(name = "position", description = "integer, default -1"),
    related_udf(name = "st_setpoint"),
    related_udf(name = "st_removepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AddPoint;

impl AddPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AddPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AddPoint {
    fn name(&self) -> &str {
        "st_addpoint"
    }

    fn signature(&self) -> &Signature {
        &ADD_POINT_SIGNATURE
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
        coerce_args(self.name(), arg_types, ADD_POINT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(add_point_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Replace point of a linestring with a given point.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the LINESTRING with the point at the 0-based position replaced; a negative position counts back from the end, -1 being the last point. The point takes the line's dimension (a missing Z or M is 0). As in PostGIS, the SRIDs aren't compared.",
    syntax_example = "ST_SetPoint(linestring, zerobasedposition, point)",
    argument(name = "linestring", description = "geometry"),
    argument(name = "zerobasedposition", description = "integer"),
    argument(name = "point", description = "geometry"),
    related_udf(name = "st_addpoint"),
    related_udf(name = "st_removepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SetPoint;

impl SetPoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SetPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for SetPoint {
    fn name(&self) -> &str {
        "st_setpoint"
    }

    fn signature(&self) -> &Signature {
        &SET_POINT_SIGNATURE
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
        coerce_args(self.name(), arg_types, SET_POINT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(set_point_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Remove a point from a linestring.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the LINESTRING without the point at the 0-based offset. A line of two points can't lose one, as in PostGIS.",
    syntax_example = "ST_RemovePoint(linestring, offset)",
    argument(name = "linestring", description = "geometry"),
    argument(name = "offset", description = "integer"),
    related_udf(name = "st_addpoint"),
    related_udf(name = "st_setpoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct RemovePoint;

impl RemovePoint {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RemovePoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for RemovePoint {
    fn name(&self) -> &str {
        "st_removepoint"
    }

    fn signature(&self) -> &Signature {
        &REMOVE_POINT_SIGNATURE
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
        coerce_args(self.name(), arg_types, REMOVE_POINT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(remove_point_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn add_point_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let kernel = EditKernel::Add {
        point: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
        position: optional_int_arg(&args, 2, -1)?,
    };
    edit_impl(args, kernel)
}

fn set_point_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let kernel = EditKernel::Set {
        position: optional_int_arg(&args, 1, 0)?,
        point: OwnedColumn::try_new(&args.args[2], &args.arg_fields[2], args.number_rows)?,
    };
    edit_impl(args, kernel)
}

fn remove_point_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let kernel = EditKernel::Remove {
        offset: optional_int_arg(&args, 1, 0)?,
    };
    edit_impl(args, kernel)
}

fn edit_impl(args: ScalarFunctionArgs, kernel: EditKernel) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

enum EditKernel {
    Add {
        point: OwnedColumn,
        position: Int32Array,
    },
    Set {
        position: Int32Array,
        point: OwnedColumn,
    },
    Remove {
        offset: Int32Array,
    },
}

impl EditKernel {
    fn name(&self) -> &'static str {
        match self {
            EditKernel::Add { .. } => "st_addpoint",
            EditKernel::Set { .. } => "st_setpoint",
            EditKernel::Remove { .. } => "st_removepoint",
        }
    }
}

impl GeometryKernel for EditKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let name = self.name();
        // SQL NULL in any argument, SQL NULL out.
        let (point, index) = match self {
            EditKernel::Add { point, position } => (Some(point.get(row)), position),
            EditKernel::Set { position, point } => (Some(point.get(row)), position),
            EditKernel::Remove { offset } => (None, offset),
        };
        if index.is_null(row) || point == Some(None) {
            return Ok(None);
        }
        let index = i64::from(index.value(row));
        let GeometryType::LineString(line) = geom.as_type() else {
            return Err(exec_datafusion_err!("{name}: First argument must be a LINESTRING").into());
        };
        let dim = dimension(geom.dim());
        let (mut coords, _) = owned_line_string(line, dim).into_inner();
        // The point's coordinate in the line's dimension, or `None` for an empty point.
        let coord = match point.flatten() {
            None => None,
            Some(Wkt::Point(point)) => point.coord().map(|c| Coord {
                x: c.x,
                y: c.y,
                z: has_ordinate(dim, true).then(|| c.z.unwrap_or(0.0)),
                m: has_ordinate(dim, false).then(|| c.m.unwrap_or(0.0)),
            }),
            Some(_) => {
                return Err(exec_datafusion_err!("{name}: Second argument must be a POINT").into());
            }
        };
        let count = coords.len() as i64;
        match self {
            EditKernel::Add { .. } => {
                let position = match index {
                    -1 => count,
                    0.. if index <= count => index,
                    _ => {
                        return Err(exec_datafusion_err!("{name}: Invalid offset").into());
                    }
                };
                if let Some(coord) = coord {
                    coords.insert(position as usize, coord);
                }
            }
            EditKernel::Set { .. } => {
                if count == 0 {
                    return Err(exec_datafusion_err!("{name}: Line has no points").into());
                }
                let position = if index < 0 { count + index } else { index };
                if !(0..count).contains(&position) {
                    return Err(exec_datafusion_err!(
                        "{name}: abs(Point index) out of range (-)(0..{})",
                        count - 1
                    )
                    .into());
                }
                let Some(coord) = coord else {
                    return Err(exec_datafusion_err!("{name}: the point is empty").into());
                };
                coords[position as usize] = coord;
            }
            EditKernel::Remove { .. } => {
                if count < 3 {
                    return Err(exec_datafusion_err!(
                        "{name}: Can't remove points from a single segment line"
                    )
                    .into());
                }
                if !(0..count).contains(&index) {
                    return Err(exec_datafusion_err!(
                        "{name}: Point index out of range (0..{})",
                        count - 1
                    )
                    .into());
                }
                coords.remove(index as usize);
            }
        }
        Ok(Some(Wkt::LineString(LineString::new(coords, dim))))
    }
}

/// Whether dimension `dim` has Z (`z`) or M (`!z`).
fn has_ordinate(dim: wkt::types::Dimension, z: bool) -> bool {
    use wkt::types::Dimension::*;
    matches!((dim, z), (XYZ | XYZM, true) | (XYM | XYZM, false))
}
