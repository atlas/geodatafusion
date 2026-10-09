use std::sync::LazyLock;

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{GeometryTrait, GeometryType};
use wkt::Wkt;
use wkt::types::{LineString, Polygon};

use crate::error::GeoDataFusionResult;
use crate::util::field::{geometries_from_array, geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::owned::{ToOwned, dimension, owned_line_string};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_MakePolygon(geometry linestring)
/// - ST_MakePolygon(geometry outerlinestring, geometry[] interiorlinestrings)
///
/// PostGIS doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::GeometryArray]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Creates a Polygon from a shell and optional list of holes.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns a POLYGON with the given LINESTRING as its shell, and an array of LINESTRINGs as its holes. Each ring must be closed (in 2D) and have at least 4 points, and all must have one dimension. A NULL in the array is an error, as in PostGIS. Unlike PostGIS, the SRIDs of the holes can't be checked: DataFusion's arrays drop them.",
    syntax_example = "ST_MakePolygon(outerlinestring, interiorlinestrings)",
    argument(name = "outerlinestring", description = "geometry, a LINESTRING"),
    argument(
        name = "interiorlinestrings",
        description = "geometry[] of LINESTRINGs"
    ),
    related_udf(name = "st_polygon"),
    related_udf(name = "st_buildarea")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakePolygon;

impl MakePolygon {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MakePolygon {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakePolygon {
    fn name(&self) -> &str {
        "st_makepolygon"
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
        Ok(make_polygon_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn make_polygon_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let holes = match args.args.get(1) {
        None => None,
        Some(holes) => Some(HoleLists::try_new(holes.to_array(args.number_rows)?)?),
    };
    let kernel = MakePolygonKernel { holes };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// The holes of every row: `None` for a NULL list, and `None` for a NULL element.
struct HoleLists {
    rows: Vec<Option<Vec<Option<Wkt<f64>>>>>,
}

impl HoleLists {
    fn try_new(array: ArrayRef) -> GeoDataFusionResult<Self> {
        let lists: Vec<Option<ArrayRef>> = match array.data_type() {
            DataType::List(_) => array.as_list::<i32>().iter().collect(),
            DataType::LargeList(_) => array.as_list::<i64>().iter().collect(),
            // A NULL literal list.
            _ => vec![None; array.len()],
        };
        let rows = lists
            .into_iter()
            .map(|list| {
                list.map(|values| {
                    let field = Field::new("", values.data_type().clone(), true);
                    let geometries = geometries_from_array(&values, &field)?;
                    map_geometry(geometries.as_ref(), &ToOwned)
                })
                .transpose()
            })
            .collect::<GeoDataFusionResult<_>>()?;
        Ok(Self { rows })
    }
}

struct MakePolygonKernel {
    holes: Option<HoleLists>,
}

impl GeometryKernel for MakePolygonKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let holes: &[Option<Wkt<f64>>] = match &self.holes {
            None => &[],
            Some(holes) => match holes.rows.get(row).and_then(Option::as_ref) {
                Some(holes) => holes,
                // SQL NULL in, SQL NULL out.
                None => return Ok(None),
            },
        };
        let GeometryType::LineString(shell) = geom.as_type() else {
            return Err(exec_datafusion_err!("st_makepolygon: Shell is not a line").into());
        };
        let dim = dimension(geom.dim());
        let mut rings = vec![owned_line_string(shell, dim)];
        for (index, hole) in holes.iter().enumerate() {
            let Some(Wkt::LineString(hole)) = hole else {
                return Err(
                    exec_datafusion_err!("st_makepolygon: Hole {index} is not a line").into(),
                );
            };
            rings.push(hole.clone());
        }
        Ok(Some(Wkt::Polygon(polygon_from_rings(
            "st_makepolygon",
            rings,
            dim,
        )?)))
    }
}

/// A polygon from its rings, with PostGIS's checks: closed in 2D, at least 4 points, and one
/// dimension.
pub(crate) fn polygon_from_rings(
    name: &str,
    rings: Vec<LineString<f64>>,
    dim: wkt::types::Dimension,
) -> GeoDataFusionResult<Polygon<f64>> {
    for (index, ring) in rings.iter().enumerate() {
        let role = if index == 0 { "shell" } else { "holes" };
        let coords = ring.coords();
        if coords.len() < 4 {
            return Err(exec_datafusion_err!("{name}: {role} must have at least 4 points").into());
        }
        let (first, last) = (&coords[0], &coords[coords.len() - 1]);
        if first.x != last.x || first.y != last.y {
            return Err(exec_datafusion_err!("{name}: {role} must be closed").into());
        }
        if ring.dimension() != dim {
            return Err(exec_datafusion_err!("{name}: mixed dimensioned rings").into());
        }
    }
    Ok(Polygon::new(rings, dim))
}
