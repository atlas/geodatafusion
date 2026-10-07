//! The spatial predicates: ST_Intersects, ST_Contains and the others that test a DE-9IM
//! relationship between two geometries.

use std::sync::LazyLock;

use arrow_schema::DataType;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geos::{Geom, Geometry, PreparedGeometry};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{GeosColumn, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::field::{common_metadata, geometry_array};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_Intersects(geometry geom1, geometry geom2), and the same for every predicate.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Declares a spatial predicate UDF: the standard UDF anatomy, evaluating `$predicate`.
// rustfmt indents the `#[user_doc]` attribute further on every run inside a macro.
#[rustfmt::skip]
macro_rules! impl_predicate_udf {
    ($struct_name:ident, $udf_name:literal, $predicate:expr, $summary:literal, $doc_text:literal, $doc_example:literal) => {
        #[doc = $summary]
        #[user_doc(
            doc_section(label = "Spatial Relationships"),
            description = $doc_text,
            syntax_example = $doc_example,
            argument(name = "geom1", description = "geometry"),
            argument(name = "geom2", description = "geometry")
        )]
        #[derive(Debug, Eq, PartialEq, Hash)]
        pub struct $struct_name;

        impl $struct_name {
            pub fn new() -> Self {
                Self
            }
        }

        impl Default for $struct_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ScalarUDFImpl for $struct_name {
            fn name(&self) -> &str {
                $udf_name
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
                Ok(predicate_impl(self.name(), args, $predicate)?)
            }

            fn documentation(&self) -> Option<&Documentation> {
                self.doc()
            }
        }
    };
}

impl_predicate_udf!(
    Intersects,
    "st_intersects",
    Predicate::Intersects,
    "Tests if two geometries intersect (they have at least one point in common).",
    "Returns true if two geometries intersect. Geometries intersect if they have any point in common. Returns false when either geometry is empty.",
    "ST_Intersects(geom1, geom2)"
);

impl_predicate_udf!(
    Disjoint,
    "st_disjoint",
    Predicate::Disjoint,
    "Tests if two geometries have no points in common.",
    "Returns true if two geometries are disjoint. Geometries are disjoint if they have no point in common. Returns true when either geometry is empty.",
    "ST_Disjoint(geom1, geom2)"
);

impl_predicate_udf!(
    Contains,
    "st_contains",
    Predicate::Contains,
    "Tests if every point of B lies in A, and their interiors have a point in common.",
    "Returns TRUE if geom1 contains geom2. A contains B if and only if all points of B lie inside (i.e. in the interior or boundary of) A (or equivalently, no points of B lie in the exterior of A), and the interiors of A and B have at least one point in common. Returns false when either geometry is empty.",
    "ST_Contains(geom1, geom2)"
);

impl_predicate_udf!(
    ContainsProperly,
    "st_containsproperly",
    Predicate::ContainsProperly,
    "Tests if every point of B lies in the interior of A.",
    "Returns true if every point of B lies in the interior of A (or equivalently, no point of B lies in the boundary or exterior of A). A does not properly contain itself, but does contain itself. Returns false when either geometry is empty.",
    "ST_ContainsProperly(geom1, geom2)"
);

impl_predicate_udf!(
    Within,
    "st_within",
    Predicate::Within,
    "Tests if every point of A lies in B, and their interiors have a point in common.",
    "Returns TRUE if geom1 is within geom2. A is within B if and only if all points of A lie inside (i.e. in the interior or boundary of) B (or equivalently, no points of A lie in the exterior of B), and the interiors of A and B have at least one point in common. Returns false when either geometry is empty.",
    "ST_Within(geom1, geom2)"
);

impl_predicate_udf!(
    Covers,
    "st_covers",
    Predicate::Covers,
    "Tests if every point of B lies in A.",
    "Returns true if every point in Geometry/Geography B lies inside (i.e. intersects the interior or boundary of) Geometry/Geography A. Equivalently, tests that no point of B lies outside (in the exterior of) A. Returns false when either geometry is empty.",
    "ST_Covers(geom1, geom2)"
);

impl_predicate_udf!(
    CoveredBy,
    "st_coveredby",
    Predicate::CoveredBy,
    "Tests if every point of A lies in B.",
    "Returns true if every point in Geometry/Geography A lies inside (i.e. intersects the interior or boundary of) Geometry/Geography B. Equivalently, tests that no point of A lies outside (in the exterior of) B. Returns false when either geometry is empty.",
    "ST_CoveredBy(geom1, geom2)"
);

impl_predicate_udf!(
    Crosses,
    "st_crosses",
    Predicate::Crosses,
    "Tests if two geometries have some, but not all, interior points in common.",
    "Compares two geometry objects and returns true if their intersection \"spatially crosses\"; that is, the geometries have some, but not all interior points in common. The intersection of the interiors of the geometries must be non-empty and must have dimension less than the maximum dimension of the two input geometries, and the intersection of the two geometries must not equal either geometry. Otherwise, it returns false. The crosses relation is symmetric and irreflexive. Returns false when either geometry is empty.",
    "ST_Crosses(geom1, geom2)"
);

impl_predicate_udf!(
    Overlaps,
    "st_overlaps",
    Predicate::Overlaps,
    "Tests if two geometries have the same dimension and intersect, but each has at least one point not in the other.",
    "Returns TRUE if geom1 and geom2 \"spatially overlap\". Two geometries overlap if they have the same dimension, their interiors intersect in that dimension. and each has at least one point inside the other (or equivalently, neither one covers the other). The overlaps relation is symmetric and irreflexive. Returns false when either geometry is empty.",
    "ST_Overlaps(geom1, geom2)"
);

impl_predicate_udf!(
    Touches,
    "st_touches",
    Predicate::Touches,
    "Tests if two geometries have at least one point in common, but their interiors do not intersect.",
    "Returns TRUE if A and B intersect, but their interiors do not intersect. Equivalently, A and B have at least one point in common, and the common points lie in at least one boundary. For Point/Point inputs the relationship is always FALSE, since points do not have a boundary. Returns false when either geometry is empty.",
    "ST_Touches(geom1, geom2)"
);

impl_predicate_udf!(
    Equals,
    "st_equals",
    Predicate::Equals,
    "Tests if two geometries include the same set of points.",
    "Returns true if the given geometries are \"topologically equal\". Use this for a 'better' answer than '='. Topological equality means that the geometries have the same dimension, and their point-sets occupy the same space. This means that the order of vertices may be different in topologically equal geometries. Returns false when either geometry is empty (true if both are).",
    "ST_Equals(geom1, geom2)"
);

#[derive(Debug, Clone, Copy)]
enum Predicate {
    Intersects,
    Disjoint,
    Contains,
    ContainsProperly,
    Within,
    Covers,
    CoveredBy,
    Crosses,
    Overlaps,
    Touches,
    Equals,
}

impl Predicate {
    /// PostGIS's result when an input is EMPTY, without calling GEOS: false, except ST_Disjoint
    /// (true) and ST_Equals (true when both are EMPTY).
    fn empty_result(self, empty1: bool, empty2: bool) -> bool {
        match self {
            Predicate::Disjoint => true,
            Predicate::Equals => empty1 && empty2,
            _ => false,
        }
    }

    /// The predicate of `(geom1, geom2)`.
    fn eval(self, geom1: &Geometry, geom2: &Geometry) -> GeoDataFusionResult<bool> {
        Ok(match self {
            Predicate::Intersects => geom1.intersects(geom2)?,
            Predicate::Disjoint => geom1.disjoint(geom2)?,
            Predicate::Contains => geom1.contains(geom2)?,
            Predicate::ContainsProperly => geom1.to_prepared_geom()?.contains_properly(geom2)?,
            Predicate::Within => geom1.within(geom2)?,
            Predicate::Covers => geom1.covers(geom2)?,
            Predicate::CoveredBy => geom1.covered_by(geom2)?,
            Predicate::Crosses => geom1.crosses(geom2)?,
            Predicate::Overlaps => geom1.overlaps(geom2)?,
            Predicate::Touches => geom1.touches(geom2)?,
            Predicate::Equals => geom1.equals(geom2)?,
        })
    }

    /// The predicate of `(geom1, geom2)` with a prepared geom1.
    fn eval_prepared1(
        self,
        geom1: &PreparedGeometry<'_>,
        geom1_raw: &Geometry,
        geom2: &Geometry,
    ) -> GeoDataFusionResult<bool> {
        Ok(match self {
            Predicate::Intersects => geom1.intersects(geom2)?,
            Predicate::Disjoint => geom1.disjoint(geom2)?,
            Predicate::Contains => geom1.contains(geom2)?,
            Predicate::ContainsProperly => geom1.contains_properly(geom2)?,
            Predicate::Within => geom1.within(geom2)?,
            Predicate::Covers => geom1.covers(geom2)?,
            Predicate::CoveredBy => geom1.covered_by(geom2)?,
            Predicate::Crosses => geom1.crosses(geom2)?,
            Predicate::Overlaps => geom1.overlaps(geom2)?,
            Predicate::Touches => geom1.touches(geom2)?,
            // GEOS has no prepared equality.
            Predicate::Equals => geom1_raw.equals(geom2)?,
        })
    }

    /// The predicate of `(geom1, geom2)` with a prepared geom2, through the converse predicate
    /// where the relationship isn't symmetric.
    fn eval_prepared2(
        self,
        geom1: &Geometry,
        geom2: &PreparedGeometry<'_>,
        geom2_raw: &Geometry,
    ) -> GeoDataFusionResult<bool> {
        Ok(match self {
            Predicate::Intersects => geom2.intersects(geom1)?,
            Predicate::Disjoint => geom2.disjoint(geom1)?,
            Predicate::Contains => geom2.within(geom1)?,
            // GEOS has no prepared converse of ST_ContainsProperly.
            Predicate::ContainsProperly => {
                geom1.to_prepared_geom()?.contains_properly(geom2_raw)?
            }
            Predicate::Within => geom2.contains(geom1)?,
            Predicate::Covers => geom2.covered_by(geom1)?,
            Predicate::CoveredBy => geom2.covers(geom1)?,
            Predicate::Crosses => geom2.crosses(geom1)?,
            Predicate::Overlaps => geom2.overlaps(geom1)?,
            Predicate::Touches => geom2.touches(geom1)?,
            Predicate::Equals => geom1.equals(geom2_raw)?,
        })
    }
}

fn predicate_impl(
    name: &str,
    args: ScalarFunctionArgs,
    predicate: Predicate,
) -> GeoDataFusionResult<ColumnarValue> {
    common_metadata(name, &args, &[0, 1])?;
    // A constant geometry is converted and prepared once, and the other argument iterated.
    let constant1 = matches!(args.args[0], ColumnarValue::Scalar(_))
        && !matches!(args.args[1], ColumnarValue::Scalar(_));
    let (rows, constant) = if constant1 { (1, 0) } else { (0, 1) };
    let geometries = geometry_array(&args, rows)?;
    let other = GeosColumn::try_new(
        &args.args[constant],
        &args.arg_fields[constant],
        args.number_rows,
    )?;
    let prepared = match &args.args[constant] {
        ColumnarValue::Scalar(_) => other
            .get(0)
            .map(|(_, geom)| geom.to_prepared_geom())
            .transpose()?,
        ColumnarValue::Array(_) => None,
    };
    let kernel = PredicateKernel {
        predicate,
        other: &other,
        prepared,
        rows_are_geom1: !constant1,
    };
    let result: arrow_array::BooleanArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(std::sync::Arc::new(result)))
}

struct PredicateKernel<'a> {
    predicate: Predicate,
    /// The other argument.
    other: &'a GeosColumn,
    /// The other argument prepared, when it's a constant.
    prepared: Option<PreparedGeometry<'a>>,
    /// Whether the iterated geometries are geom1 (and `other` is geom2).
    rows_are_geom1: bool,
}

impl GeometryKernel for PredicateKernel<'_> {
    type Output = bool;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<bool>> {
        // The predicates are STRICT: SQL NULL in any argument gives SQL NULL.
        let Some((other_input, other)) = self.other.get(row) else {
            return Ok(None);
        };
        // PostGIS answers for EMPTY inputs without calling GEOS.
        let (empty, other_empty) = (
            is_geometry_topologically_empty(geom),
            is_geometry_topologically_empty(other_input),
        );
        if empty || other_empty {
            let (empty1, empty2) = if self.rows_are_geom1 {
                (empty, other_empty)
            } else {
                (other_empty, empty)
            };
            return Ok(Some(self.predicate.empty_result(empty1, empty2)));
        }
        let geom = to_geos(geom)?;
        let result = match (&self.prepared, self.rows_are_geom1) {
            (Some(prepared), true) => self.predicate.eval_prepared2(&geom, prepared, other)?,
            (Some(prepared), false) => self.predicate.eval_prepared1(prepared, other, &geom)?,
            (None, true) => self.predicate.eval(&geom, other)?,
            (None, false) => self.predicate.eval(other, &geom)?,
        };
        Ok(Some(result))
    }
}
