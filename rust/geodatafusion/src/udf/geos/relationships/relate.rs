//! ST_Relate.

use std::sync::{Arc, LazyLock};

use arrow_array::{BooleanArray, StringArray};
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geos::{Geom, Geometry};
use wkt::Wkt;
use wkt::types::{GeometryCollection, MultiLineString, MultiPoint, MultiPolygon};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::udf::native::relationships::relate_match::relate_match;
use crate::util::args::optional_text_arg;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Relate(geometry geom1, geometry geom2) and
/// ST_Relate(geometry geom1, geometry geom2, text intersectionMatrixPattern). The
/// ST_Relate(geometry, geometry, integer boundaryNodeRule) overload isn't supported: the geos
/// crate doesn't bind GEOSRelateBoundaryNodeRule.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Text],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2", "intersectionMatrixPattern"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the DE-9IM matrix of two geometries, or tests it against a pattern.
#[user_doc(
    doc_section(label = "Spatial Relationships"),
    description = "Returns the DE-9IM intersection matrix of two geometries as a 9-character string, or, with an intersectionMatrixPattern, whether the matrix matches it (see ST_RelateMatch). The boundaryNodeRule overload is not supported.",
    syntax_example = "ST_Relate(geom1, geom2, intersectionMatrixPattern)",
    argument(name = "geom1", description = "geometry"),
    argument(name = "geom2", description = "geometry"),
    argument(name = "intersectionMatrixPattern", description = "text"),
    related_udf(name = "st_relatematch")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Relate;

impl Relate {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Relate {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Relate {
    fn name(&self) -> &str {
        "st_relate"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, arg_types: &[DataType]) -> Result<DataType> {
        // The matrix, or whether it matches the pattern.
        Ok(if arg_types.len() == 3 {
            DataType::Boolean
        } else {
            DataType::Utf8
        })
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(relate_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn relate_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    let geometries = geometry_array(&args, 0)?;
    let kernel = RelateKernel {
        other: GeosColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
    };
    let matrices: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    if args.args.len() < 3 {
        return Ok(ColumnarValue::Array(Arc::new(matrices)));
    }
    let patterns = optional_text_arg(&args, 2, "")?;
    let result = matrices
        .iter()
        .zip(patterns.iter())
        .map(|(matrix, pattern)| match (matrix, pattern) {
            // SQL NULL in, SQL NULL out.
            (Some(matrix), Some(pattern)) => relate_match(matrix, pattern).map(Some),
            _ => Ok(None),
        })
        .collect::<GeoDataFusionResult<BooleanArray>>()?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct RelateKernel {
    other: GeosColumn,
}

impl GeometryKernel for RelateKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // ST_Relate is STRICT: SQL NULL in any argument gives SQL NULL.
        let Some((other_input, other)) = self.other.get(row) else {
            return Ok(None);
        };
        let empty = is_geometry_topologically_empty(geom);
        let other_empty = is_geometry_topologically_empty(other_input);
        if !empty && !other_empty {
            return Ok(Some(to_geos(geom)?.relate(other)?));
        }
        // GEOS 3.14 segfaults relating POLYGON EMPTY to a collection holding LINESTRING EMPTY.
        // An EMPTY geometry relates to anything like POINT EMPTY does, and EMPTY parts of the
        // other geometry don't change the matrix, so relate those instead.
        let empty_point = Geometry::create_empty_point()?;
        let matrix = match (empty, other_empty) {
            (true, true) => empty_point.relate(&empty_point)?,
            (true, false) => empty_point.relate(&to_geos(&without_empty_parts(other_input))?)?,
            (false, _) => {
                to_geos(&without_empty_parts(&to_owned_geometry(geom)))?.relate(&empty_point)?
            }
        };
        Ok(Some(matrix))
    }
}

/// A geometry without the EMPTY parts of its collections.
fn without_empty_parts(geom: &Wkt<f64>) -> Wkt<f64> {
    match geom {
        Wkt::MultiPoint(points) => Wkt::MultiPoint(MultiPoint::new(
            points
                .points()
                .iter()
                .filter(|point| point.coord().is_some())
                .cloned()
                .collect(),
            points.dimension(),
        )),
        Wkt::MultiLineString(lines) => Wkt::MultiLineString(MultiLineString::new(
            lines
                .line_strings()
                .iter()
                .filter(|line| !line.coords().is_empty())
                .cloned()
                .collect(),
            lines.dimension(),
        )),
        Wkt::MultiPolygon(polygons) => Wkt::MultiPolygon(MultiPolygon::new(
            polygons
                .polygons()
                .iter()
                .filter(|polygon| !polygon.rings().is_empty())
                .cloned()
                .collect(),
            polygons.dimension(),
        )),
        Wkt::GeometryCollection(collection) => Wkt::GeometryCollection(GeometryCollection::new(
            collection
                .geometries()
                .iter()
                .filter(|member| !is_geometry_topologically_empty(*member))
                .map(without_empty_parts)
                .collect(),
            collection.dimension(),
        )),
        other => other.clone(),
    }
}

#[cfg(test)]
mod test {
    use arrow_array::cast::AsArray;
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;

    /// GEOS segfaults on these pairs, so the PostGIS oracle can't record them.
    #[tokio::test]
    async fn test_relate_empty_against_collection_with_empty_parts() {
        let ctx = SessionContext::new();
        ctx.register_udf(Relate.into());
        ctx.register_udf(GeomFromText::new().into());

        for (empty, expected) in [("POINT EMPTY", "FFFFFF0F2"), ("POLYGON EMPTY", "FFFFFF0F2")] {
            let sql = format!(
                "SELECT ST_Relate(ST_GeomFromText('{empty}'), \
                 ST_GeomFromText('GEOMETRYCOLLECTION(LINESTRING EMPTY, POINT(1 1))')), \
                 ST_Relate(ST_GeomFromText('GEOMETRYCOLLECTION(LINESTRING EMPTY, POINT(1 1))'), \
                 ST_GeomFromText('{empty}'))"
            );
            let batches = ctx.sql(&sql).await.unwrap().collect().await.unwrap();
            assert_eq!(
                batches[0].column(0).as_string::<i32>().value(0),
                expected,
                "{empty}"
            );
            assert_eq!(
                batches[0].column(1).as_string::<i32>().value(0),
                "FF0FFFFF2",
                "{empty}"
            );
        }
    }
}
