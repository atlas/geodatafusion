//! ST_ClipByBox2D.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, GeometryTrait, GeometryType, RectTrait};
use geos::Geom;
use wkt::Wkt;
use wkt::types::Dimension;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{empty_of_type, from_geos, has_z, to_geos};
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::util::float_box::serialized_box;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry, map_geometry_to_wkb};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};

/// PostGIS: ST_ClipByBox2D(geometry geom, box2d box). A geometry for the box stands for its
/// bounding box, as PostgreSQL casts it to box2d.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Geometry, Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "box"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Computes the portion of a geometry falling within a rectangle.
#[user_doc(
    doc_section(label = "Overlay Functions"),
    description = "Clips a geometry by a 2D box in a fast and tolerant but possibly invalid way: topologically invalid input doesn't raise an error, and the result may be invalid too, as with GEOS's rectangle clipping, which PostGIS uses. An empty geometry is returned as it is, and one whose bounding box (as PostGIS stores it, in single precision for geometries larger than a point or a two-point line) doesn't touch the box gives a 2D empty geometry of its type; otherwise a box without area gives NULL. A geometry strictly inside the box is returned as it is. Z is kept and interpolated, and M dropped, where the geometry is clipped; a clip GEOS fails on, as on some invalid input, gives NULL, as in PostGIS. A geometry for the box stands for its bounding box.",
    syntax_example = "ST_ClipByBox2D(geom, box)",
    argument(name = "geom", description = "geometry"),
    argument(name = "box", description = "box2d")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct ClipByBox2D;

impl ClipByBox2D {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipByBox2D {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for ClipByBox2D {
    fn name(&self) -> &str {
        "st_clipbybox2d"
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
        Ok(clip_by_box2d_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn clip_by_box2d_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let boxes = geometry_array(&args, 1)?;
    let kernel = ClipKernel {
        boxes: map_geometry(boxes.as_ref(), &BoxKernel)?,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

/// A box: `[min_x, min_y, max_x, max_y]`.
type Box2D = [f64; 4];

/// Reads a `box2d`, or the box PostgreSQL casts a geometry to: its exact bounding box.
struct BoxKernel;

impl GeometryKernel for BoxKernel {
    type Output = Box2D;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<Box2D>> {
        if let GeometryType::Rect(rect) = geom.as_type() {
            let (min, max) = (rect.min(), rect.max());
            return Ok(Some([min.x(), min.y(), max.x(), max.y()]));
        }
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        Ok((!rect.is_empty()).then(|| [rect.minx(), rect.miny(), rect.maxx(), rect.maxy()]))
    }
}

struct ClipKernel {
    boxes: Vec<Option<Box2D>>,
}

impl GeometryKernel for ClipKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // A NULL or empty box gives NULL.
        let Some([min_x, min_y, max_x, max_y]) = self.boxes.get(row).copied().flatten() else {
            return Ok(None);
        };
        // An empty geometry is returned as it is.
        let Some([gx0, gy0, gx1, gy1]) = serialized_box(geom) else {
            return Ok(Some(to_owned_geometry(geom)));
        };
        // PostGIS checks the box it stores with the geometry first: if it is apart from the box,
        // the result is a 2D empty geometry of its type.
        if gx0 > max_x || gx1 < min_x || gy0 > max_y || gy1 < min_y {
            return Ok(Some(empty_of_type(geom, Dimension::XY)));
        }
        // Then a box without area gives NULL.
        if min_x >= max_x || min_y >= max_y {
            return Ok(None);
        }
        // GEOS returns a geometry strictly inside the box as it is, M and all.
        let mut envelope = BoundingRect::new(false);
        envelope.add_geometry(geom);
        if envelope.minx() > min_x
            && envelope.maxx() < max_x
            && envelope.miny() > min_y
            && envelope.maxy() < max_y
        {
            return Ok(Some(to_owned_geometry(geom)));
        }
        // A clip GEOS fails on, such as one leaving a ring too few points, gives NULL, as in
        // PostGIS.
        let Ok(clipped) = to_geos(geom)?.clip_by_rect(min_x, min_y, max_x, max_y) else {
            return Ok(None);
        };
        Ok(Some(from_geos(&clipped, has_z(geom))?))
    }
}
