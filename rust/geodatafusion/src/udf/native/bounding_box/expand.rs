use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, Dimensions, GeometryTrait, GeometryType, RectTrait};
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::RectBuilder;
use geoarrow_schema::{Dimension, GeoArrowType};
use wkt::Wkt;
use wkt::types::{Coord, LineString, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::ordinates::z;
use crate::util::owned::{dimension, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_Expand(geometry geom, float units_to_expand)
/// - ST_Expand(geometry geom, float dx, float dy, float dz = 0, float dm = 0)
/// - ST_Expand(box2d box, float units_to_expand)
/// - ST_Expand(box2d box, float dx, float dy)
/// - ST_Expand(box3d box, float units_to_expand)
/// - ST_Expand(box3d box, float dx, float dy, float dz = 0)
///
/// The box overloads are told apart from the geometry ones by the argument's type, when planning.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Float],
    &[
        Arg::Geometry,
        Arg::Float,
        Arg::Float,
        Arg::Float,
        Arg::Float,
    ],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "dx", "dy", "dz", "dm"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a bounding box expanded from another bounding box or a geometry.
#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "Returns the bounding box of a geometry or box, expanded by units_to_expand in every dimension, or by dx, dy, dz and dm in each. A box2d or box3d gives a box of the same type. A geometry gives its expanded box as a POLYGON, even when it is degenerate, with Z and M if the geometry has them; an empty geometry is returned unchanged. Negative distances shrink the box, and can invert it.",
    syntax_example = "ST_Expand(geom, units_to_expand)",
    alternative_syntax = "ST_Expand(geom, dx, dy, dz, dm)",
    argument(name = "geom", description = "geometry, box2d or box3d"),
    argument(
        name = "dx",
        description = "float8, the distance in every dimension (units_to_expand) when it is the only one"
    ),
    argument(name = "dy", description = "float8"),
    argument(name = "dz", description = "float8, default 0"),
    argument(name = "dm", description = "float8, default 0, geometry only"),
    related_udf(name = "st_envelope")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Expand;

impl Expand {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Expand {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Expand {
    fn name(&self) -> &str {
        "st_expand"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        expand_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(expand_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// A box argument gives a box of the same type, with PostGIS's arities for it; a geometry gives
/// WKB.
fn expand_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let field = &args.arg_fields[0];
    let Ok(GeoArrowType::Rect(box_type)) = GeoArrowType::from_arrow_field(field) else {
        return Ok(wkb_return_field(name, input_metadata(field)));
    };
    let arity = args.arg_fields.len();
    let supported = match box_type.dimension() {
        Dimension::XY => arity <= 3,
        _ => arity <= 4,
    };
    if !supported {
        let sql_type = match box_type.dimension() {
            Dimension::XY => "box2d",
            _ => "box3d",
        };
        return plan_err!(
            "{name} does not support a {sql_type} with {} distances",
            arity - 1
        );
    }
    Ok(Arc::new(box_type.to_field(name, true)))
}

fn expand_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let kernel = ExpandKernel::try_new(&args)?;
    match GeoArrowType::from_arrow_field(&args.return_field)? {
        GeoArrowType::Rect(box_type) => {
            let include_z = box_type.dimension() != Dimension::XY;
            let boxes: Vec<Option<ExpandedBox>> = map_geometry(geometries.as_ref(), &kernel)?;
            let mut builder = RectBuilder::with_capacity(box_type, boxes.len());
            for expanded in &boxes {
                match expanded {
                    Some(expanded) => {
                        let (min, max) = expanded.corners(include_z);
                        builder.push_min_max(&min, &max);
                    }
                    None => builder.push_null(),
                }
            }
            Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
        }
        GeoArrowType::Wkb(_) => {
            let result = map_geometry_to_wkb(
                geometries.as_ref(),
                &ExpandGeometryKernel(kernel),
                &args.return_field,
            )?;
            Ok(ColumnarValue::Array(result.to_array_ref()))
        }
        other => {
            Err(internal_datafusion_err!("st_expand: unexpected return type {other:?}").into())
        }
    }
}

/// The distances to expand by, one per row. With one distance, it is the distance in every
/// dimension.
struct ExpandKernel {
    dx: Float64Array,
    dy: Float64Array,
    dz: Float64Array,
    dm: Float64Array,
}

impl ExpandKernel {
    fn try_new(args: &ScalarFunctionArgs) -> Result<Self> {
        let dx = optional_float_arg(args, 1, 0.0)?;
        if args.args.len() == 2 {
            return Ok(Self {
                dy: dx.clone(),
                dz: dx.clone(),
                dm: dx.clone(),
                dx,
            });
        }
        Ok(Self {
            dx,
            dy: optional_float_arg(args, 2, 0.0)?,
            dz: optional_float_arg(args, 3, 0.0)?,
            dm: optional_float_arg(args, 4, 0.0)?,
        })
    }

    /// The distances in row `row`, or `None` if any is NULL.
    fn distances(&self, row: usize) -> Option<[f64; 4]> {
        let arrays = [&self.dx, &self.dy, &self.dz, &self.dm];
        if arrays.iter().any(|array| array.is_null(row)) {
            return None;
        }
        Some(arrays.map(|array| array.value(row)))
    }
}

/// The ranges of an expanded box: x, y, and Z and M if the input has them. A range isn't
/// reordered when a negative distance inverts it, as in PostGIS.
struct ExpandedBox {
    x: (f64, f64),
    y: (f64, f64),
    z: Option<(f64, f64)>,
    m: Option<(f64, f64)>,
}

impl ExpandedBox {
    fn corners(&self, include_z: bool) -> (Coord<f64>, Coord<f64>) {
        let z = include_z.then(|| self.z.unwrap_or((0.0, 0.0)));
        let corner = |pick: fn((f64, f64)) -> f64| Coord {
            x: pick(self.x),
            y: pick(self.y),
            z: z.map(pick),
            m: None,
        };
        (corner(|range| range.0), corner(|range| range.1))
    }
}

fn grow((min, max): (f64, f64), distance: f64) -> (f64, f64) {
    (min - distance, max + distance)
}

impl GeometryKernel for ExpandKernel {
    type Output = ExpandedBox;

    /// Expands a box2d or box3d argument; its bounds are read as they are, so an inverted box
    /// stays inverted.
    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<ExpandedBox>> {
        let Some([dx, dy, dz, _]) = self.distances(row) else {
            return Ok(None);
        };
        let GeometryType::Rect(rect) = geom.as_type() else {
            return Err(
                internal_datafusion_err!("st_expand: a box argument holds a geometry").into(),
            );
        };
        let (min, max) = (rect.min(), rect.max());
        Ok(Some(ExpandedBox {
            x: grow((min.x(), max.x()), dx),
            y: grow((min.y(), max.y()), dy),
            z: z(&min).zip(z(&max)).map(|range| grow(range, dz)),
            m: None,
        }))
    }
}

struct ExpandGeometryKernel(ExpandKernel);

impl GeometryKernel for ExpandGeometryKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some([dx, dy, dz, dm]) = self.0.distances(row) else {
            return Ok(None);
        };
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        if rect.is_empty() {
            return Ok(Some(to_owned_geometry(geom)));
        }
        let dim = geom.dim();
        let (has_z, has_m) = match dim {
            Dimensions::Xyz => (true, false),
            Dimensions::Xym => (false, true),
            Dimensions::Xyzm => (true, true),
            Dimensions::Xy | Dimensions::Unknown(_) => (false, false),
        };
        let expanded = ExpandedBox {
            x: grow((rect.minx(), rect.maxx()), dx),
            y: grow((rect.miny(), rect.maxy()), dy),
            z: rect
                .z_range()
                .filter(|_| has_z)
                .map(|range| grow(range, dz)),
            m: rect
                .m_range()
                .filter(|_| has_m)
                .map(|range| grow(range, dm)),
        };
        Ok(Some(box_polygon(&expanded, dim)))
    }
}

/// The polygon PostGIS makes of a box: (xmin ymin, xmin ymax, xmax ymax, xmax ymin), closed, with
/// the minimum Z and M on the first two corners and the maximum on the last two. Degenerate and
/// inverted boxes keep all five points.
fn box_polygon(expanded: &ExpandedBox, dim: Dimensions) -> Wkt<f64> {
    let (x, y) = (expanded.x, expanded.y);
    let low = |x, y| Coord {
        x,
        y,
        z: expanded.z.map(|z| z.0),
        m: expanded.m.map(|m| m.0),
    };
    let high = |x, y| Coord {
        x,
        y,
        z: expanded.z.map(|z| z.1),
        m: expanded.m.map(|m| m.1),
    };
    let ring = vec![
        low(x.0, y.0),
        low(x.0, y.1),
        high(x.1, y.1),
        high(x.1, y.0),
        low(x.0, y.0),
    ];
    let dim = dimension(dim);
    Wkt::Polygon(Polygon::new(vec![LineString::new(ring, dim)], dim))
}
