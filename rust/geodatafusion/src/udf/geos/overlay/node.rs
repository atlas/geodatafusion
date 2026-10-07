//! ST_Node.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, LineStringTrait, MultiLineStringTrait};
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, LineString, MultiLineString};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{empty_like, from_geos, has_z, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Node(geometry geom).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Nodes a set of lines.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Returns a (multi)linestring representing the fully noded version of a (multi)linestring: lines are split at every intersection, and shared portions collapse to one. Other geometry types are an error. This function keeps Z and drops M.",
    syntax_example = "ST_Node(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Node;

impl Node {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Node {
    fn name(&self) -> &str {
        "st_node"
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
        Ok(node_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn node_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = NodeKernel;
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct NodeKernel;

impl GeometryKernel for NodeKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        // PostGIS checks the input type itself, and only nodes linestrings.
        match geom.as_type() {
            geo_traits::GeometryType::LineString(_)
            | geo_traits::GeometryType::MultiLineString(_) => {}
            geo_traits::GeometryType::GeometryCollection(_) => {
                return Err(
                    exec_datafusion_err!("st_node: geometry collections are unsupported").into(),
                );
            }
            _ => {
                return Err(exec_datafusion_err!(
                    "st_node: Noding geometries of dimension != 1 is unsupported"
                )
                .into());
            }
        }
        // PostGIS returns EMPTY input unchanged.
        if is_geometry_topologically_empty(geom) {
            return Ok(Some(empty_like(geom)));
        }
        // PostGIS's noding: union the linework (which nodes it), merge it back into the longest
        // lines, and split those again at the endpoints of the input lines.
        let merged = to_geos(geom)?.unary_union()?.line_merge()?;
        let endpoints = endpoints(geom);
        let lines = match from_geos(&merged, want_z)? {
            Wkt::LineString(line) => vec![line],
            Wkt::MultiLineString(lines) => lines.into_inner().0,
            other => return Ok(Some(other)),
        };
        let mut parts = lines
            .into_iter()
            .flat_map(|line| split_at(line, &endpoints))
            .collect::<Vec<_>>();
        Ok(Some(if parts.len() == 1 {
            Wkt::LineString(parts.remove(0))
        } else {
            let dim = parts.first().map_or(Dimension::XY, LineString::dimension);
            Wkt::MultiLineString(MultiLineString::new(parts, dim))
        }))
    }
}

/// The XY of the first and last points of every line of a (multi)linestring.
fn endpoints(geom: &impl GeometryTrait<T = f64>) -> Vec<(f64, f64)> {
    let mut endpoints = vec![];
    match geom.as_type() {
        geo_traits::GeometryType::LineString(line) => add_ends(line, &mut endpoints),
        geo_traits::GeometryType::MultiLineString(lines) => {
            for line in lines.line_strings() {
                add_ends(&line, &mut endpoints);
            }
        }
        _ => {}
    }
    endpoints
}

fn add_ends(line: &impl LineStringTrait<T = f64>, endpoints: &mut Vec<(f64, f64)>) {
    let ends = [line.coord(0), line.coord(line.num_coords().wrapping_sub(1))];
    endpoints.extend(ends.into_iter().flatten().map(|c| (c.x(), c.y())));
}

/// Splits a line at every interior vertex that is one of `endpoints`.
fn split_at(line: LineString<f64>, endpoints: &[(f64, f64)]) -> Vec<LineString<f64>> {
    let (coords, dim) = line.into_inner();
    let mut parts = vec![];
    let mut current = vec![];
    let last = coords.len().saturating_sub(1);
    for (i, coord) in coords.into_iter().enumerate() {
        current.push(coord);
        if i > 0 && i < last && endpoints.contains(&(coord.x, coord.y)) {
            parts.push(LineString::new(std::mem::take(&mut current), dim));
            current.push(coord);
        }
    }
    parts.push(LineString::new(current, dim));
    parts
}
