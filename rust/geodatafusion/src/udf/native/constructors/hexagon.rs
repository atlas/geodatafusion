//! ST_Hexagon and ST_Square: a cell of a hexagonal or square grid.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::GeoArrowType;
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString, Polygon};

use crate::error::GeoDataFusionResult;
use crate::util::args::{optional_float_arg, optional_int_arg};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Hexagon(float8 size, integer cell_i, integer cell_j, geometry origin = 'POINT(0 0)'),
/// and the same for ST_Square.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Integer, Arg::Integer],
    &[Arg::Float, Arg::Integer, Arg::Integer, Arg::Geometry],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["size", "cell_i", "cell_j", "origin"])
        .expect("parameter names are valid for a user-defined signature")
});

/// cos(π/6), as PostGIS's hexagon heights show it: one unit in the last place above the double
/// nearest √3/2.
const HEXAGON_HEIGHT: f64 = 0.8660254037844387;

/// Returns a single hexagon, using the provided edge size and cell coordinate within the hexagon
/// grid space.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns the hexagon at cell (cell_i, cell_j) of a grid of flat-topped hexagons with edges of the given size, centred on the origin POINT (default POINT(0 0)) at cell (0, 0), with odd columns shifted up by half a hexagon. The result has the origin's SRID.",
    syntax_example = "ST_Hexagon(size, cell_i, cell_j, origin)",
    argument(name = "size", description = "float8"),
    argument(name = "cell_i", description = "integer"),
    argument(name = "cell_j", description = "integer"),
    argument(name = "origin", description = "geometry, default POINT(0 0)"),
    related_udf(name = "st_square"),
    related_udf(name = "st_hexagongrid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Hexagon;

impl Hexagon {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Hexagon {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Hexagon {
    fn name(&self) -> &str {
        "st_hexagon"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cell_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cell_impl(self.name(), args, Grid::Hexagon)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a single square, using the provided edge size and cell coordinate within the square
/// grid space.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns the square at cell (cell_i, cell_j) of a grid of squares with edges of the given size, whose cell (0, 0) has its lower left corner at the origin POINT (default POINT(0 0)). The result has the origin's SRID.",
    syntax_example = "ST_Square(size, cell_i, cell_j, origin)",
    argument(name = "size", description = "float8"),
    argument(name = "cell_i", description = "integer"),
    argument(name = "cell_j", description = "integer"),
    argument(name = "origin", description = "geometry, default POINT(0 0)"),
    related_udf(name = "st_hexagon"),
    related_udf(name = "st_squaregrid")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Square;

impl Square {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Square {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Square {
    fn name(&self) -> &str {
        "st_square"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cell_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cell_impl(self.name(), args, Grid::Square)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// WKB with the origin's CRS, or none.
fn cell_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let metadata = args
        .arg_fields
        .get(3)
        .map(|origin| input_metadata(origin))
        .unwrap_or_default();
    Ok(wkb_return_field(name, metadata))
}

#[derive(Debug, Clone, Copy)]
enum Grid {
    Hexagon,
    Square,
}

fn cell_impl(
    name: &str,
    args: ScalarFunctionArgs,
    grid: Grid,
) -> GeoDataFusionResult<ColumnarValue> {
    let size = optional_float_arg(&args, 0, 0.0)?;
    let cell_i = optional_int_arg(&args, 1, 0)?;
    let cell_j = optional_int_arg(&args, 2, 0)?;
    let origin = match args.args.get(3) {
        Some(origin) => Some(OwnedColumn::try_new(
            origin,
            &args.arg_fields[3],
            args.number_rows,
        )?),
        None => None,
    };
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(exec_datafusion_err!("{name}: expected a WKB return field").into());
    };
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for row in 0..args.number_rows {
        let cell = cell(name, grid, &size, &cell_i, &cell_j, origin.as_ref(), row)?;
        builder.push_geometry(cell.as_ref())?;
    }
    Ok(ColumnarValue::Array(builder.finish().to_array_ref()))
}

/// The cell in row `row`, or `None` if an argument is NULL.
fn cell(
    name: &str,
    grid: Grid,
    size: &Float64Array,
    cell_i: &Int32Array,
    cell_j: &Int32Array,
    origin: Option<&OwnedColumn>,
    row: usize,
) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    if size.is_null(row) || cell_i.is_null(row) || cell_j.is_null(row) {
        return Ok(None);
    }
    let (ox, oy) = match origin {
        None => (0.0, 0.0),
        Some(origin) => match origin.get(row) {
            None => return Ok(None),
            Some(Wkt::Point(point)) => match point.coord() {
                Some(coord) => (coord.x, coord.y),
                None => {
                    return Err(exec_datafusion_err!("{name}: origin point is empty").into());
                }
            },
            Some(_) => {
                return Err(exec_datafusion_err!("{name}: origin argument is not a point").into());
            }
        },
    };
    let (size, i, j) = (size.value(row), cell_i.value(row), cell_j.value(row));
    let corner = |x, y| Coord {
        x,
        y,
        z: None,
        m: None,
    };
    let ring: Vec<Coord<f64>> = match grid {
        // Vertices from the left, counter-clockwise, as unit offsets scaled by the size, which is
        // how PostGIS's coordinates round.
        Grid::Hexagon => {
            const OFFSETS: [(f64, f64); 7] = [
                (-1.0, 0.0),
                (-0.5, -1.0),
                (0.5, -1.0),
                (1.0, 0.0),
                (0.5, 1.0),
                (-0.5, 1.0),
                (-1.0, 0.0),
            ];
            let column = 1.5 * f64::from(i);
            // Odd columns are shifted up half a hexagon.
            let row_offset = 2.0 * f64::from(j) + f64::from(i.rem_euclid(2));
            let height = size * HEXAGON_HEIGHT;
            OFFSETS
                .iter()
                .map(|(dx, dy)| corner(ox + size * (column + dx), oy + height * (row_offset + dy)))
                .collect()
        }
        Grid::Square => {
            let (i, j) = (f64::from(i), f64::from(j));
            let (xmin, ymin) = (ox + size * i, oy + size * j);
            let (xmax, ymax) = (ox + size * (i + 1.0), oy + size * (j + 1.0));
            vec![
                corner(xmin, ymin),
                corner(xmin, ymax),
                corner(xmax, ymax),
                corner(xmax, ymin),
                corner(xmin, ymin),
            ]
        }
    };
    Ok(Some(Wkt::Polygon(Polygon::new(
        vec![LineString::new(ring, Dimension::XY)],
        Dimension::XY,
    ))))
}
