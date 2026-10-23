//! The geography operators: the box predicate behind `&&` and the distance behind `<->`.

use std::sync::{Arc, LazyLock};

use arrow_array::{BooleanArray, Float64Array};
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geocentric_box::GeocentricBox;
use crate::udf::native::util::geodesic::{GeodeticArgument, geography_pair};
use crate::util::field::geometries_from_array;
use crate::util::geography::geography_argument;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: geography_overlaps(geography, geography) and geography_distance_knn(geography,
/// geography). Geometry is cast to geography, as PostgreSQL does.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geog1", "geog2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Tests if the geocentric bounding boxes of two geographies intersect.
#[user_doc(
    doc_section(label = "Operators"),
    description = "Returns TRUE if the bounding box of A intersects the bounding box of B. Geography's boxes are geocentric, on the unit sphere, and take in where edges bulge between their ends along great circles, so LINESTRING(-10 60, 10 60) and POINT(0 60) don't overlap. A polygon's box reaches the sphere along an axis when its rings straddle the two other axes, whether or not the polygon encloses that point. Boxes are rounded outward to single precision, as PostGIS stores them. SRIDs aren't checked. False when either is EMPTY.",
    syntax_example = "geography_overlaps(geog1, geog2)",
    alternative_syntax = "geog1 && geog2",
    argument(name = "geog1", description = "geography"),
    argument(name = "geog2", description = "geography")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeographyOverlaps;

impl GeographyOverlaps {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeographyOverlaps {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeographyOverlaps {
    fn name(&self) -> &str {
        "geography_overlaps"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geography_overlaps_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geography_overlaps_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let (first, first_field, _) = geography_argument(name, &args, 0)?;
    let (second, second_field, _) = geography_argument(name, &args, 1)?;
    let geometries = geometries_from_array(&first.to_array(args.number_rows)?, &first_field)?;
    let kernel = OverlapsKernel {
        geom2: OwnedColumn::try_new(&second, &second_field, args.number_rows)?,
    };
    let result: BooleanArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct OverlapsKernel {
    geom2: OwnedColumn,
}

impl GeometryKernel for OverlapsKernel {
    type Output = bool;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        let Some(geom2) = self.geom2.get(row) else {
            return Ok(None);
        };
        let overlaps = match (GeocentricBox::new(geom1), GeocentricBox::new(geom2)) {
            (Some(a), Some(b)) => a.overlaps(&b),
            _ => false,
        };
        Ok(Some(overlaps))
    }
}

/// Returns the distance between two geographies on the sphere.
#[user_doc(
    doc_section(label = "Operators"),
    description = "Returns the distance between two geographies in metres, on the sphere of the spheroid's mean radius, as ST_Distance(geog1, geog2, false) measures it but not rounded. Returns NULL if either is EMPTY. Their SRIDs must match.",
    syntax_example = "geography_distance_knn(geog1, geog2)",
    alternative_syntax = "geog1 <-> geog2",
    argument(name = "geog1", description = "geography"),
    argument(name = "geog2", description = "geography")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeographyDistanceKnn;

impl GeographyDistanceKnn {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeographyDistanceKnn {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeographyDistanceKnn {
    fn name(&self) -> &str {
        "geography_distance_knn"
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
        Ok(geography_distance_knn_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn geography_distance_knn_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let use_spheroid = BooleanArray::from(vec![false; args.number_rows]);
    let pair = geography_pair(name, &args, use_spheroid)?;
    let kernel = SphereDistanceKernel {
        first: &pair.first,
        geom2: OwnedColumn::try_new(&pair.second.0, &pair.second.1, args.number_rows)?,
    };
    let result: Float64Array = map_geometry(pair.first.geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct SphereDistanceKernel<'a> {
    first: &'a GeodeticArgument,
    geom2: OwnedColumn,
}

impl GeometryKernel for SphereDistanceKernel<'_> {
    type Output = f64;

    fn eval(
        &self,
        geom1: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<f64>> {
        Ok(self
            .geom2
            .get(row)
            .and_then(|geom2| self.first.geodesic.geometry_distance(geom1, geom2, false)))
    }
}
