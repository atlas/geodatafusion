use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Float64Array, Int32Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, Metadata};
use wkt::Wkt;
use wkt::types::{Coord, Dimension, LineString, Polygon};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::util::args::{optional_float_arg, optional_int_arg};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::owned::OwnedColumn;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_TileEnvelope(integer zoom, integer x, integer y, geometry bounds = (the Web
/// Mercator extent, SRID 3857), float margin = 0.0).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Integer, Arg::Integer, Arg::Integer],
    &[Arg::Integer, Arg::Integer, Arg::Integer, Arg::Geometry],
    &[
        Arg::Integer,
        Arg::Integer,
        Arg::Integer,
        Arg::Geometry,
        Arg::Float,
    ],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["zoom", "x", "y", "bounds", "margin"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Half the width of the Web Mercator extent PostGIS's default bounds cover.
const WEB_MERCATOR_HALF_WIDTH: f64 = 20037508.342789244;

/// The largest zoom PostGIS accepts.
const MAX_ZOOM: i32 = 31;

/// Creates a rectangular Polygon in Web Mercator (SRID:3857) using the XYZ tile system.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Returns the POLYGON of tile (x, y) at zoom level zoom (0 to 31) in the XYZ tile system: the bounds are split into 2^zoom by 2^zoom tiles, numbered from the top left. The bounds default to the Web Mercator extent with SRID 3857, and give the result its SRID. margin grows the tile by that fraction of its size on every side (shrinks it when negative, down to -0.5), clipped to the bounds.",
    syntax_example = "ST_TileEnvelope(zoom, x, y, bounds, margin)",
    argument(name = "zoom", description = "integer"),
    argument(name = "x", description = "integer"),
    argument(name = "y", description = "integer"),
    argument(
        name = "bounds",
        description = "geometry, default the Web Mercator extent"
    ),
    argument(name = "margin", description = "float8, default 0"),
    related_udf(name = "st_makeenvelope")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct TileEnvelope;

impl TileEnvelope {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TileEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for TileEnvelope {
    fn name(&self) -> &str {
        "st_tileenvelope"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let metadata = match args.arg_fields.get(3) {
            Some(bounds) => input_metadata(bounds),
            None => Arc::new(Metadata::new(srid_to_crs(3857), None)),
        };
        Ok(wkb_return_field(self.name(), metadata))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(tile_envelope_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn tile_envelope_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let zoom = optional_int_arg(&args, 0, 0)?;
    let x = optional_int_arg(&args, 1, 0)?;
    let y = optional_int_arg(&args, 2, 0)?;
    let bounds = match args.args.get(3) {
        Some(bounds) => Some(OwnedColumn::try_new(
            bounds,
            &args.arg_fields[3],
            args.number_rows,
        )?),
        None => None,
    };
    let margin = optional_float_arg(&args, 4, 0.0)?;
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(exec_datafusion_err!("{name}: expected a WKB return field").into());
    };
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for row in 0..args.number_rows {
        let tile = tile(name, &zoom, &x, &y, bounds.as_ref(), &margin, row)?;
        builder.push_geometry(tile.as_ref())?;
    }
    Ok(ColumnarValue::Array(builder.finish().to_array_ref()))
}

/// The tile polygon in row `row`, or `None` if an argument is NULL.
fn tile(
    name: &str,
    zoom: &Int32Array,
    x: &Int32Array,
    y: &Int32Array,
    bounds: Option<&OwnedColumn>,
    margin: &Float64Array,
    row: usize,
) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    if [zoom, x, y].iter().any(|array| array.is_null(row)) || margin.is_null(row) {
        return Ok(None);
    }
    let (xmin, ymin, xmax, ymax) = match bounds {
        None => (
            -WEB_MERCATOR_HALF_WIDTH,
            -WEB_MERCATOR_HALF_WIDTH,
            WEB_MERCATOR_HALF_WIDTH,
            WEB_MERCATOR_HALF_WIDTH,
        ),
        Some(bounds) => {
            let Some(bounds) = bounds.get(row) else {
                return Ok(None);
            };
            let mut rect = BoundingRect::new(false);
            rect.add_geometry(bounds);
            if rect.is_empty() {
                return Err(exec_datafusion_err!("{name}: Unable to compute bbox").into());
            }
            if rect.minx() == rect.maxx() || rect.miny() == rect.maxy() {
                return Err(exec_datafusion_err!("{name}: Geometric bounds are too small").into());
            }
            (rect.minx(), rect.miny(), rect.maxx(), rect.maxy())
        }
    };
    let (zoom, x, y, margin) = (
        zoom.value(row),
        x.value(row),
        y.value(row),
        margin.value(row),
    );
    if !(0..=MAX_ZOOM).contains(&zoom) {
        return Err(exec_datafusion_err!("{name}: Invalid tile zoom value, {zoom}").into());
    }
    let tiles = 1i64 << zoom;
    if !(0..tiles).contains(&i64::from(x)) {
        return Err(exec_datafusion_err!("{name}: Invalid tile x value, {x}").into());
    }
    if !(0..tiles).contains(&i64::from(y)) {
        return Err(exec_datafusion_err!("{name}: Invalid tile y value, {y}").into());
    }
    if margin < -0.5 {
        return Err(exec_datafusion_err!(
            "{name}: Margin must not be less than -50%, margin={margin:.6}"
        )
        .into());
    }
    let tiles = tiles as f64;
    let (width, height) = ((xmax - xmin) / tiles, (ymax - ymin) / tiles);
    let (x, y) = (f64::from(x), f64::from(y));
    // Tiles are numbered from the top left; the margin is clipped to the bounds.
    let tile_xmin = (xmin + width * (x - margin)).max(xmin);
    let tile_xmax = (xmin + width * (x + 1.0 + margin)).min(xmax);
    let tile_ymax = (ymax - height * (y - margin)).min(ymax);
    let tile_ymin = (ymax - height * (y + 1.0 + margin)).max(ymin);
    let corner = |x, y| Coord {
        x,
        y,
        z: None,
        m: None,
    };
    let ring = vec![
        corner(tile_xmin, tile_ymin),
        corner(tile_xmin, tile_ymax),
        corner(tile_xmax, tile_ymax),
        corner(tile_xmax, tile_ymin),
        corner(tile_xmin, tile_ymin),
    ];
    Ok(Some(Wkt::Polygon(Polygon::new(
        vec![LineString::new(ring, Dimension::XY)],
        Dimension::XY,
    ))))
}
