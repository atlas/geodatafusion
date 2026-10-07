//! Voronoi diagrams: ST_VoronoiPolygons and ST_VoronoiLines.

use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, GeometryCollection};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, from_geos, has_z, to_geos};
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_VoronoiPolygons(geometry g1, float8 tolerance = 0, geometry extend_to = NULL),
/// and the same for ST_VoronoiLines.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Geometry],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["g1", "tolerance", "extend_to"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the cells of the Voronoi diagram of a geometry's vertices.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a collection of the polygons of the Voronoi diagram of the vertices of a geometry. Vertices closer than tolerance are merged. The diagram extends over the envelope of extend_to if given, otherwise over the input's envelope expanded by half its size. Fewer than two vertices give an empty collection. The result is 2D.",
    syntax_example = "ST_VoronoiPolygons(g1, tolerance, extend_to)",
    argument(name = "g1", description = "geometry"),
    argument(name = "tolerance", description = "float8"),
    argument(name = "extend_to", description = "geometry"),
    related_udf(name = "st_voronoilines")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct VoronoiPolygons;

impl VoronoiPolygons {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VoronoiPolygons {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for VoronoiPolygons {
    fn name(&self) -> &str {
        "st_voronoipolygons"
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
        Ok(voronoi_impl(self.name(), args, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns the edges of the Voronoi diagram of a geometry's vertices.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Returns a multilinestring of the edges of the Voronoi diagram of the vertices of a geometry. Vertices closer than tolerance are merged. The diagram extends over the envelope of extend_to if given, otherwise over the input's envelope expanded by half its size. Fewer than two vertices give an empty collection. The result is 2D.",
    syntax_example = "ST_VoronoiLines(g1, tolerance, extend_to)",
    argument(name = "g1", description = "geometry"),
    argument(name = "tolerance", description = "float8"),
    argument(name = "extend_to", description = "geometry"),
    related_udf(name = "st_voronoipolygons")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct VoronoiLines;

impl VoronoiLines {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VoronoiLines {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for VoronoiLines {
    fn name(&self) -> &str {
        "st_voronoilines"
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
        Ok(voronoi_impl(self.name(), args, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn voronoi_impl(
    name: &str,
    args: ScalarFunctionArgs,
    only_edges: bool,
) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let extend_to = match (args.args.get(2), args.arg_fields.get(2)) {
        (Some(value), Some(field)) => Some(GeosColumn::try_new(value, field, args.number_rows)?),
        _ => None,
    };
    let kernel = VoronoiKernel {
        name,
        only_edges,
        tolerance: optional_float_arg(&args, 1, 0.0)?,
        extend_to,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct VoronoiKernel<'a> {
    name: &'a str,
    only_edges: bool,
    tolerance: Float64Array,
    extend_to: Option<GeosColumn>,
}

impl GeometryKernel for VoronoiKernel<'_> {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // PostGIS keeps a Z from GEOS only when the input has Z.
        let want_z = has_z(geom);
        // Not STRICT: a NULL extend_to means the default envelope, and a NULL tolerance is an
        // error.
        let tolerance = (!self.tolerance.is_null(row))
            .then(|| self.tolerance.value(row))
            .filter(|tolerance| *tolerance >= 0.0)
            .ok_or_else(|| {
                exec_datafusion_err!("{}: Tolerance must be a positive number.", self.name)
            })?;
        let geom = to_geos(geom)?;
        // PostGIS returns an empty collection for fewer than two vertices.
        if geom.get_num_coordinates()? < 2 {
            return Ok(Some(Wkt::GeometryCollection(GeometryCollection::empty(
                Dimension::XY,
            ))));
        }
        let extend_to = self
            .extend_to
            .as_ref()
            .and_then(|column| column.get(row))
            .map(|(_, extend_to)| extend_to);
        let diagram = geom.voronoi(extend_to, tolerance, self.only_edges)?;
        Ok(Some(from_geos(&diagram, want_z)?))
    }
}
