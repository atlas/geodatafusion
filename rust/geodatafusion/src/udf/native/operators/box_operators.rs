//! The bounding box operators: the predicates behind `&&`, `&&&`, `~`, `@`, `~=`, `<<`, `&<`,
//! `>>`, `&>`, `<<|`, `&<|`, `|>>` and `|&>`, and the box distance behind `<#>`.
//!
//! PostGIS compares the single-precision boxes its index stores, rounded outward from the double
//! coordinates, so `'POINT(1 1)' ~= 'POINT(1.00000001 1)'` is false. These compute the same
//! boxes. An EMPTY geometry has no box: the overlap and position predicates are false for it,
//! the containment predicates true, and `~=` is true only when both are EMPTY.

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
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::util::float_box::FloatBox;
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: geometry_overlaps(geometry geom1, geometry geom2), and the same for every operator.
/// Boxes are accepted too, as PostGIS's box2df and gidx overloads do.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom1", "geom2"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Declares a bounding box predicate UDF: the standard UDF anatomy, evaluating `$predicate`.
// rustfmt indents the `#[user_doc]` attribute further on every run inside a macro.
#[rustfmt::skip]
macro_rules! impl_box_predicate_udf {
    ($struct_name:ident, $udf_name:literal, $predicate:expr, $summary:literal, $doc_text:literal, $doc_example:literal, $operator_example:literal) => {
        #[doc = $summary]
        #[user_doc(
            doc_section(label = "Operators"),
            description = $doc_text,
            syntax_example = $doc_example,
            alternative_syntax = $operator_example,
            argument(name = "geom1", description = "geometry or box"),
            argument(name = "geom2", description = "geometry or box")
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
                Ok(box_predicate_impl(args, $predicate)?)
            }

            fn documentation(&self) -> Option<&Documentation> {
                self.doc()
            }
        }
    };
}

impl_box_predicate_udf!(
    GeometryOverlaps,
    "geometry_overlaps",
    BoxPredicate::Overlaps,
    "Tests if the 2D bounding boxes of two geometries intersect.",
    "Returns TRUE if the 2D bounding box of A intersects the 2D bounding box of B. Unlike ST_Intersects, it compares boxes only, and SRIDs aren't checked. False when either is EMPTY.",
    "geometry_overlaps(geom1, geom2)",
    "geom1 && geom2"
);

impl_box_predicate_udf!(
    GeometryOverlapsNd,
    "geometry_overlaps_nd",
    BoxPredicate::OverlapsNd,
    "Tests if the n-D bounding boxes of two geometries intersect.",
    "Returns TRUE if the n-D bounding box of A intersects the n-D bounding box of B. X and Y are always compared, Z when both have Z, and M when both have M. False when either is EMPTY.",
    "geometry_overlaps_nd(geom1, geom2)",
    "geom1 &&& geom2"
);

impl_box_predicate_udf!(
    GeometryContains,
    "geometry_contains",
    BoxPredicate::Contains,
    "Tests if the bounding box of A contains the bounding box of B.",
    "Returns TRUE if the bounding box of A completely contains the bounding box of B. True when either is EMPTY, as in PostGIS.",
    "geometry_contains(geom1, geom2)",
    "geom1 ~ geom2"
);

impl_box_predicate_udf!(
    GeometryWithin,
    "geometry_within",
    BoxPredicate::Within,
    "Tests if the bounding box of A is contained by the bounding box of B.",
    "Returns TRUE if the bounding box of A is completely contained by the bounding box of B. True when either is EMPTY, as in PostGIS.",
    "geometry_within(geom1, geom2)",
    "geom1 @ geom2"
);

impl_box_predicate_udf!(
    GeometrySame,
    "geometry_same",
    BoxPredicate::Same,
    "Tests if two geometries have the same bounding box.",
    "Returns TRUE if the bounding box of A is the same as the bounding box of B. It doesn't compare the geometries: LINESTRING(0 0, 1 1) and LINESTRING(0 1, 1 0) are the same. True for two EMPTY geometries, false for one.",
    "geometry_same(geom1, geom2)",
    "geom1 ~= geom2"
);

impl_box_predicate_udf!(
    GeometryLeft,
    "geometry_left",
    BoxPredicate::Left,
    "Tests if the bounding box of A is strictly to the left of the bounding box of B.",
    "Returns TRUE if the bounding box of A is strictly to the left of the bounding box of B. False when either is EMPTY.",
    "geometry_left(geom1, geom2)",
    "geom1 << geom2"
);

impl_box_predicate_udf!(
    GeometryOverLeft,
    "geometry_overleft",
    BoxPredicate::OverLeft,
    "Tests if the bounding box of A overlaps or is to the left of the bounding box of B.",
    "Returns TRUE if the bounding box of A overlaps or is to the left of the bounding box of B: the right edge of A isn't right of the right edge of B. False when either is EMPTY.",
    "geometry_overleft(geom1, geom2)",
    "geom1 &< geom2"
);

impl_box_predicate_udf!(
    GeometryRight,
    "geometry_right",
    BoxPredicate::Right,
    "Tests if the bounding box of A is strictly to the right of the bounding box of B.",
    "Returns TRUE if the bounding box of A is strictly to the right of the bounding box of B. False when either is EMPTY.",
    "geometry_right(geom1, geom2)",
    "geom1 >> geom2"
);

impl_box_predicate_udf!(
    GeometryOverRight,
    "geometry_overright",
    BoxPredicate::OverRight,
    "Tests if the bounding box of A overlaps or is to the right of the bounding box of B.",
    "Returns TRUE if the bounding box of A overlaps or is to the right of the bounding box of B: the left edge of A isn't left of the left edge of B. False when either is EMPTY.",
    "geometry_overright(geom1, geom2)",
    "geom1 &> geom2"
);

impl_box_predicate_udf!(
    GeometryBelow,
    "geometry_below",
    BoxPredicate::Below,
    "Tests if the bounding box of A is strictly below the bounding box of B.",
    "Returns TRUE if the bounding box of A is strictly below the bounding box of B. False when either is EMPTY.",
    "geometry_below(geom1, geom2)",
    "geom1 <<| geom2"
);

impl_box_predicate_udf!(
    GeometryOverBelow,
    "geometry_overbelow",
    BoxPredicate::OverBelow,
    "Tests if the bounding box of A overlaps or is below the bounding box of B.",
    "Returns TRUE if the bounding box of A overlaps or is below the bounding box of B: the top edge of A isn't above the top edge of B. False when either is EMPTY.",
    "geometry_overbelow(geom1, geom2)",
    "geom1 &<| geom2"
);

impl_box_predicate_udf!(
    GeometryAbove,
    "geometry_above",
    BoxPredicate::Above,
    "Tests if the bounding box of A is strictly above the bounding box of B.",
    "Returns TRUE if the bounding box of A is strictly above the bounding box of B. False when either is EMPTY.",
    "geometry_above(geom1, geom2)",
    "geom1 |>> geom2"
);

impl_box_predicate_udf!(
    GeometryOverAbove,
    "geometry_overabove",
    BoxPredicate::OverAbove,
    "Tests if the bounding box of A overlaps or is above the bounding box of B.",
    "Returns TRUE if the bounding box of A overlaps or is above the bounding box of B: the bottom edge of A isn't below the bottom edge of B. False when either is EMPTY.",
    "geometry_overabove(geom1, geom2)",
    "geom1 |&> geom2"
);

/// Returns the 2D distance between the bounding boxes of two geometries.
#[user_doc(
    doc_section(label = "Operators"),
    description = "Returns the 2D distance between the bounding boxes of A and B: 0 when they intersect. With an EMPTY geometry, the largest single-precision float, as in PostGIS.",
    syntax_example = "geometry_distance_box(geom1, geom2)",
    alternative_syntax = "geom1 <#> geom2",
    argument(name = "geom1", description = "geometry or box"),
    argument(name = "geom2", description = "geometry or box")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeometryDistanceBox;

impl GeometryDistanceBox {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeometryDistanceBox {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeometryDistanceBox {
    fn name(&self) -> &str {
        "geometry_distance_box"
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
        Ok(geometry_distance_box_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[derive(Debug, Clone, Copy)]
enum BoxPredicate {
    Overlaps,
    OverlapsNd,
    Contains,
    Within,
    Same,
    Left,
    OverLeft,
    Right,
    OverRight,
    Below,
    OverBelow,
    Above,
    OverAbove,
}

impl BoxPredicate {
    fn eval(self, a: Option<&FloatBox>, b: Option<&FloatBox>) -> bool {
        use BoxPredicate::*;

        let (Some(a), Some(b)) = (a, b) else {
            return match self {
                Contains | Within => true,
                Same => a.is_none() && b.is_none(),
                _ => false,
            };
        };
        match self {
            Overlaps => overlap(a.x, b.x) && overlap(a.y, b.y),
            OverlapsNd => {
                overlap(a.x, b.x)
                    && overlap(a.y, b.y)
                    && a.z.zip(b.z).is_none_or(|(a, b)| overlap(a, b))
                    && a.m.zip(b.m).is_none_or(|(a, b)| overlap(a, b))
            }
            Contains => contains(a, b),
            Within => contains(b, a),
            Same => a.x == b.x && a.y == b.y,
            Left => a.x.1 < b.x.0,
            OverLeft => a.x.1 <= b.x.1,
            Right => a.x.0 > b.x.1,
            OverRight => a.x.0 >= b.x.0,
            Below => a.y.1 < b.y.0,
            OverBelow => a.y.1 <= b.y.1,
            Above => a.y.0 > b.y.1,
            OverAbove => a.y.0 >= b.y.0,
        }
    }
}

fn overlap(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 <= b.1 && a.1 >= b.0
}

fn contains(a: &FloatBox, b: &FloatBox) -> bool {
    a.x.0 <= b.x.0 && a.x.1 >= b.x.1 && a.y.0 <= b.y.0 && a.y.1 >= b.y.1
}

/// The bounding box of every geometry, keeping the box of an EMPTY geometry (which is empty) apart
/// from SQL NULL.
struct BoxKernel;

impl GeometryKernel for BoxKernel {
    type Output = BoundingRect;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<BoundingRect>> {
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        Ok(Some(rect))
    }
}

/// The boxes of argument `index`: `None` for SQL NULL, `Some(None)` for EMPTY.
fn float_boxes(
    args: &ScalarFunctionArgs,
    index: usize,
) -> GeoDataFusionResult<Vec<Option<Option<FloatBox>>>> {
    let geometries = geometry_array(args, index)?;
    let rects: Vec<Option<BoundingRect>> = map_geometry(geometries.as_ref(), &BoxKernel)?;
    Ok(rects
        .iter()
        .map(|rect| rect.as_ref().map(FloatBox::new))
        .collect())
}

fn box_predicate_impl(
    args: ScalarFunctionArgs,
    predicate: BoxPredicate,
) -> GeoDataFusionResult<ColumnarValue> {
    let a = float_boxes(&args, 0)?;
    let b = float_boxes(&args, 1)?;
    let result: BooleanArray = a
        .iter()
        .zip(&b)
        .map(|(a, b)| Some(predicate.eval(a.as_ref()?.as_ref(), b.as_ref()?.as_ref())))
        .collect();
    Ok(ColumnarValue::Array(Arc::new(result)))
}

fn geometry_distance_box_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let a = float_boxes(&args, 0)?;
    let b = float_boxes(&args, 1)?;
    let result: Float64Array = a
        .iter()
        .zip(&b)
        .map(|(a, b)| Some(box_distance(a.as_ref()?.as_ref(), b.as_ref()?.as_ref())))
        .collect();
    Ok(ColumnarValue::Array(Arc::new(result)))
}

fn box_distance(a: Option<&FloatBox>, b: Option<&FloatBox>) -> f64 {
    let (Some(a), Some(b)) = (a, b) else {
        return f32::MAX as f64;
    };
    let gap = |a: (f64, f64), b: (f64, f64)| {
        if a.1 < b.0 {
            b.0 - a.1
        } else if b.1 < a.0 {
            a.0 - b.1
        } else {
            0.0
        }
    };
    let (dx, dy) = (gap(a.x, b.x), gap(a.y, b.y));
    (dx * dx + dy * dy).sqrt()
}
