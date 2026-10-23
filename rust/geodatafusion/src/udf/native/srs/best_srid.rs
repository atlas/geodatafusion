use std::sync::{Arc, LazyLock};

use arrow_array::Int32Array;
use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::best_srid::best_srid;
use crate::udf::native::util::geocentric_box::GeocentricBox;
use crate::util::field::geometries_from_array;
use crate::util::geography::geography_argument;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: _ST_BestSRID(geography geog1, geography geog2) and _ST_BestSRID(geography geog1).
/// Geometry is cast to geography, as PostgreSQL does.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry], &[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geog1", "geog2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns the SRID of a planar projection suited to geography.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Returns the SRID of the planar projection PostGIS uses for the geography forms of ST_Buffer and ST_Intersection, chosen by the geocentric bounding box of geog1, or of both: Lambert azimuthal equal-area on the pole (999061 north, 999161 south) for boxes centred north or south of 70° and under 45° high; the UTM zone of the centre for boxes under 6° wide (999001 to 999060 north of the equator, 999101 to 999160 south); a Lambert azimuthal equal-area tile for boxes under 25° high and narrower than the tiles of their 30° band of latitude (90°, 45° or 30° wide, from the poles to the equator); and World Mercator (999000) otherwise. ST_Transform knows these SRIDs. For an EMPTY geography PostGIS's result varies from call to call; geodatafusion gives 999031.",
    syntax_example = "_ST_BestSRID(geog1)",
    alternative_syntax = "_ST_BestSRID(geog1, geog2)",
    argument(name = "geog1", description = "geography"),
    argument(name = "geog2", description = "geography")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct BestSRID;

impl BestSRID {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BestSRID {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for BestSRID {
    fn name(&self) -> &str {
        "_st_bestsrid"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Int32)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(best_srid_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn best_srid_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let (first, first_field, _) = geography_argument(name, &args, 0)?;
    let geometries = geometries_from_array(&first.to_array(args.number_rows)?, &first_field)?;
    let second = if args.args.len() > 1 {
        let (second, second_field, _) = geography_argument(name, &args, 1)?;
        Some(OwnedColumn::try_new(
            &second,
            &second_field,
            args.number_rows,
        )?)
    } else {
        None
    };
    let result: Int32Array = map_geometry(geometries.as_ref(), &BestSridKernel { second })?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct BestSridKernel {
    second: Option<OwnedColumn>,
}

impl GeometryKernel for BestSridKernel {
    type Output = i32;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<i32>> {
        let bounds = GeocentricBox::new(geom);
        let bounds = match &self.second {
            None => bounds,
            Some(second) => {
                let Some(other) = second.get(row) else {
                    return Ok(None);
                };
                match (bounds, GeocentricBox::new(other)) {
                    (Some(a), Some(b)) => Some(a.union(&b)),
                    (a, b) => a.or(b),
                }
            }
        };
        Ok(Some(best_srid(bounds)))
    }
}
