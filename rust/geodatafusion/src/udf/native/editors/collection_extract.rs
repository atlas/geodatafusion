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
use wkt::types::{
    Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
    Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::dimension::dimension as topological_dimension;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{dimension, owned_atoms, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_CollectionExtract(geometry collection)
/// - ST_CollectionExtract(geometry collection, integer type)
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Integer]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["collection", "type"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Given a geometry collection, returns a multi-geometry containing only elements of a specified
/// type.
#[user_doc(
    doc_section(label = "Geometry Editors"),
    description = "Returns the points (type 1), linestrings (2) or polygons (3) of a collection as a MULTI* geometry, without empty ones, looking into nested collections. Without a type, or with 0, the type of highest dimension is taken, and a collection without any parts gives GEOMETRYCOLLECTION EMPTY. A geometry that isn't a collection is returned as it is if it has the type, and as an empty geometry of the type otherwise.",
    syntax_example = "ST_CollectionExtract(collection, type)",
    argument(name = "collection", description = "geometry"),
    argument(name = "type", description = "integer: 1, 2 or 3"),
    related_udf(name = "st_collectionhomogenize"),
    related_udf(name = "st_multi")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CollectionExtract;

impl CollectionExtract {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CollectionExtract {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for CollectionExtract {
    fn name(&self) -> &str {
        "st_collectionextract"
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
        Ok(collection_extract_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn collection_extract_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = CollectionExtractKernel {
        // 0 means the type of highest dimension.
        types: optional_int_arg(&args, 1, 0)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct CollectionExtractKernel {
    types: Int32Array,
}

impl GeometryKernel for CollectionExtractKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.types.is_null(row) {
            return Ok(None);
        }
        // The topological dimension of the type to extract, if one is asked for.
        let wanted = match self.types.value(row) {
            0 => None,
            t @ 1..=3 => Some(t - 1),
            _ => {
                return Err(exec_datafusion_err!(
                    "st_collectionextract: only point, linestring and polygon may be extracted"
                )
                .into());
            }
        };
        let dim = dimension(geom.dim());
        let is_collection = matches!(
            geom.as_type(),
            GeometryType::MultiPoint(_)
                | GeometryType::MultiLineString(_)
                | GeometryType::MultiPolygon(_)
                | GeometryType::GeometryCollection(_)
        );
        if !is_collection {
            return Ok(Some(match wanted {
                Some(wanted) if wanted != topological_dimension(geom) => empty_single(wanted, dim),
                _ => to_owned_geometry(geom),
            }));
        }
        let atoms = owned_atoms(geom);
        let Some(wanted) = wanted.or_else(|| atoms.iter().map(topological_dimension).max()) else {
            return Ok(Some(Wkt::GeometryCollection(GeometryCollection::new(
                vec![],
                dim,
            ))));
        };
        let atoms = atoms
            .into_iter()
            .filter(|atom| topological_dimension(atom) == wanted)
            .filter(|atom| !is_geometry_topologically_empty(atom));
        Ok(Some(match wanted {
            0 => Wkt::MultiPoint(MultiPoint::new(
                atoms
                    .filter_map(|atom| match atom {
                        Wkt::Point(point) => Some(point),
                        _ => None,
                    })
                    .collect(),
                dim,
            )),
            1 => Wkt::MultiLineString(MultiLineString::new(
                atoms
                    .filter_map(|atom| match atom {
                        Wkt::LineString(line) => Some(line),
                        _ => None,
                    })
                    .collect(),
                dim,
            )),
            _ => Wkt::MultiPolygon(MultiPolygon::new(
                atoms
                    .filter_map(|atom| match atom {
                        Wkt::Polygon(polygon) => Some(polygon),
                        _ => None,
                    })
                    .collect(),
                dim,
            )),
        }))
    }
}

/// An empty POINT, LINESTRING or POLYGON, by topological dimension.
fn empty_single(topological_dimension: i32, dim: Dimension) -> Wkt<f64> {
    match topological_dimension {
        0 => Wkt::Point(Point::new(None, dim)),
        1 => Wkt::LineString(LineString::new(vec![], dim)),
        _ => Wkt::Polygon(Polygon::new(vec![], dim)),
    }
}
