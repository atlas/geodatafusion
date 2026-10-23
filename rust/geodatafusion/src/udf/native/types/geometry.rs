use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_array::{Array, new_null_array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{
    ScalarValue, exec_datafusion_err, internal_datafusion_err, internal_err, plan_err,
};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{CoordTrait, RectTrait};
use geoarrow_array::array::{WkbArray, from_arrow_array};
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::cast::{AsGeoArrowArray, to_wkb};
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor};
use geoarrow_schema::{Edges, GeoArrowType, Metadata, WkbType};

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::wkb::ewkb_srid;
use crate::udf::native::util::box_geometry::box_geometry;
use crate::util::ewkt::{ewkt_srid_prefix, parse_ewkt};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::geography::{CoerceIntoRange, geography_metadata, geography_srid, is_geography};
use crate::util::kernel::map_geometry_to_wkb;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_GEOGRAPHY_DEFAULT, SRID_UNKNOWN, crs_to_srid, srid_to_crs};

/// PostGIS: geometry(text), geometry(bytea) and geometry(box2d), geometry(box3d) and
/// geometry(geometry), which a cast to a `geometry` column type calls with its type modifier.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text], &[Arg::Bytea], &[Arg::Geometry]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Converts a value to a geometry: what `::geometry` casts call.
#[user_doc(
    doc_section(label = "Data Types"),
    description = "Converts text, bytea, a box or a geometry to a geometry, as the ::geometry cast does. Text may be WKT, EWKT or hex-encoded EWKB, and bytea is EWKB. An embedded SRID becomes the column's; a cast to geometry(type, srid) sets the SRID of geometries without one, and a different SRID is an error. Unlike PostGIS, an embedded SRID must be the same in every row, because geodatafusion stores one CRS per column, and the geometry type of a type modifier is not checked.",
    syntax_example = "geometry(geom)",
    argument(name = "geom", description = "text, bytea, box2d, box3d or geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Geometry {
    /// The SRID of the cast's target type, `geometry(type, srid)`; unknown without one.
    srid: i32,
}

impl Geometry {
    pub fn new() -> Self {
        Self { srid: SRID_UNKNOWN }
    }

    /// The conversion of a cast to `geometry(type, srid)`.
    #[cfg(feature = "sql")]
    pub(crate) fn with_srid(srid: i32) -> Self {
        Self { srid }
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Geometry {
    fn name(&self) -> &str {
        "geometry"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cast_return_field(self.name(), &args, self.srid, Target::Geometry)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cast_impl(self.name(), args, Target::Geometry, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: geography(text), geography(bytea) and geography(geometry), and geography(geography),
/// which a cast to a `geography` column type calls with its type modifier.
static GEOGRAPHY_ARGUMENTS: &[&[Arg]] = &[&[Arg::Text], &[Arg::Bytea], &[Arg::Geometry]];

/// Converts a value to a geography: what `::geography` casts call.
#[user_doc(
    doc_section(label = "Data Types"),
    description = "Converts text, bytea or a geometry to a geography, as the ::geography cast does: geometry on longitude/latitude, with edges along great circles. Text may be WKT, EWKT or hex-encoded EWKB, and bytea is EWKB. The SRID is the embedded one or the geometry's, and 4326 without one; it must be a longitude/latitude SRID. As in PostGIS, coordinates beyond the range of longitude and latitude are brought into it: longitudes by whole turns, latitudes folded back over the poles. Unlike PostGIS, an embedded SRID must be the same in every row, because geodatafusion stores one CRS per column, and the geometry type of a type modifier is not checked.",
    syntax_example = "geography(geog)",
    argument(name = "geog", description = "text, bytea or geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Geography {
    /// The SRID of the cast's target type, `geography(type, srid)`; unknown without one.
    srid: i32,
}

impl Geography {
    pub fn new() -> Self {
        Self { srid: SRID_UNKNOWN }
    }

    /// The conversion of a cast to `geography(type, srid)`.
    #[cfg(feature = "sql")]
    pub(crate) fn with_srid(srid: i32) -> Self {
        Self { srid }
    }
}

impl Default for Geography {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Geography {
    fn name(&self) -> &str {
        "geography"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cast_return_field(self.name(), &args, self.srid, Target::Geography)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, GEOGRAPHY_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cast_impl(self.name(), args, Target::Geography, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// What a cast converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Geometry,
    Geography,
}

/// The field of a cast's result: the SRID is the type modifier's, the embedded one, or the
/// input's (see [`column_srid`]); geography's is 4326 without one, and must be longitude/latitude.
pub(crate) fn cast_return_field(
    name: &str,
    args: &ReturnFieldArgs,
    typmod_srid: i32,
    target: Target,
) -> Result<FieldRef> {
    let input = &args.arg_fields[0];
    let input_srid = if is_geography(input) {
        geography_srid(input)
    } else if is_geometry(input) {
        crs_to_srid(input_metadata(input).crs())
    } else {
        Some(
            args.scalar_arguments
                .first()
                .and_then(|literal| literal_srid(*literal))
                .unwrap_or(SRID_UNKNOWN),
        )
    };
    let Some(input_srid) = input_srid else {
        // A CRS without an SRID can't be compared with a type modifier, or checked to be
        // longitude/latitude, so it's kept.
        if typmod_srid == SRID_UNKNOWN {
            let crs = input_metadata(input).crs().clone();
            let edges = (target == Target::Geography).then_some(Edges::Spherical);
            return Ok(wkb_return_field(name, Arc::new(Metadata::new(crs, edges))));
        }
        return plan_err!(
            "{name}: Geometry SRID (none) does not match column SRID ({typmod_srid})"
        );
    };
    // Geography without an SRID is 4326 before it meets the type modifier, as in PostGIS.
    let input_srid = match (target, input_srid) {
        (Target::Geography, SRID_UNKNOWN) => SRID_GEOGRAPHY_DEFAULT,
        (_, srid) => srid,
    };
    let srid = column_srid(name, input_srid, typmod_srid)?;
    let metadata = match target {
        Target::Geometry => Metadata::new(srid_to_crs(srid), None),
        Target::Geography => geography_metadata(name, srid)?,
    };
    Ok(wkb_return_field(name, Arc::new(metadata)))
}

/// Whether a field holds geometries rather than text or bytes to parse.
fn is_geometry(field: &FieldRef) -> bool {
    field
        .extension_type_name()
        .is_some_and(|name| name.starts_with("geoarrow."))
}

/// The SRID of a text or bytea literal, which becomes the column's when planning.
fn literal_srid(literal: Option<&ScalarValue>) -> Option<i32> {
    match literal? {
        ScalarValue::Utf8(Some(text))
        | ScalarValue::LargeUtf8(Some(text))
        | ScalarValue::Utf8View(Some(text)) => {
            if is_hex_ewkb(text) {
                ewkb_srid(&decode_hex(text).ok()?)
            } else {
                ewkt_srid_prefix(text)
            }
        }
        ScalarValue::Binary(Some(bytes))
        | ScalarValue::LargeBinary(Some(bytes))
        | ScalarValue::BinaryView(Some(bytes)) => ewkb_srid(bytes),
        _ => None,
    }
}

/// The SRID of the result: a geometry without an SRID takes the type modifier's, and a geometry
/// with a different SRID than the type modifier's is an error, as in PostGIS.
fn column_srid(name: &str, geometry_srid: i32, typmod_srid: i32) -> Result<i32> {
    if typmod_srid == SRID_UNKNOWN {
        return Ok(geometry_srid);
    }
    if geometry_srid != SRID_UNKNOWN && geometry_srid != typmod_srid {
        return plan_err!(
            "{name}: Geometry SRID ({geometry_srid}) does not match column SRID ({typmod_srid})"
        );
    }
    Ok(typmod_srid)
}

/// Converts the argument, as [`cast_return_field`] planned. Text is (E)WKT, or hex-encoded EWKB if
/// `hex` allows it.
pub(crate) fn cast_impl(
    name: &str,
    args: ScalarFunctionArgs,
    target: Target,
    hex: bool,
) -> GeoDataFusionResult<ColumnarValue> {
    let return_field = Arc::clone(&args.return_field);
    let result = convert(name, args, hex)?;
    if target == Target::Geometry {
        return Ok(ColumnarValue::Array(result));
    }
    let geometries = from_arrow_array(&result, &return_field)?;
    let coerced = map_geometry_to_wkb(geometries.as_ref(), &CoerceIntoRange, &return_field)?;
    Ok(ColumnarValue::Array(coerced.into_array_ref()))
}

/// The value converted to WKB with the type of the return field.
fn convert(name: &str, args: ScalarFunctionArgs, hex: bool) -> GeoDataFusionResult<Arc<dyn Array>> {
    let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(internal_datafusion_err!("{name}: unexpected return field").into());
    };
    let input = &args.arg_fields[0];
    if is_geometry(input) {
        // The column's SRID was checked when planning.
        let geometries = geometry_array(&args, 0)?;
        if let GeoArrowType::Rect(_) = geometries.data_type() {
            let mut builder = WkbBuilder::<i32>::new(output_type);
            for rect in geometries.as_rect().iter() {
                let geometry = rect
                    .transpose()?
                    .map(|rect| box_to_geometry(&rect))
                    .transpose()?;
                builder.push_geometry(geometry.as_ref())?;
            }
            return Ok(builder.finish().into_array_ref());
        }
        let wkb = to_wkb::<i32>(geometries.as_ref())?;
        let array = WkbArray::new(wkb.inner().clone(), output_type.metadata().clone());
        return Ok(array.into_array_ref());
    }
    let column_srid = crs_to_srid(output_type.metadata().crs()).unwrap_or(SRID_UNKNOWN);
    let values = args.args[0].to_array(args.number_rows)?;
    let array = match values.data_type() {
        DataType::Null => new_null_array(args.return_field.data_type(), args.number_rows),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            let texts = args.args[0]
                .cast_to(&DataType::Utf8, None)?
                .to_array(args.number_rows)?;
            parse_texts(
                name,
                texts.as_string::<i32>(),
                output_type,
                column_srid,
                hex,
            )?
        }
        _ => {
            let buffers = args.args[0]
                .cast_to(&DataType::Binary, None)?
                .to_array(args.number_rows)?;
            let mut builder = WkbBuilder::<i32>::new(output_type);
            for buffer in buffers.as_binary::<i32>() {
                match buffer {
                    Some(buffer) => push_ewkb(name, &mut builder, buffer, column_srid)?,
                    None => builder.push_geometry(None::<&wkt::Wkt<f64>>)?,
                }
            }
            builder.finish().into_array_ref()
        }
    };
    Ok(array)
}

fn box_to_geometry(rect: &impl RectTrait<T = f64>) -> Result<wkt::Wkt<f64>> {
    let axes = 0..rect.dim().size();
    let min: Vec<f64> = axes
        .clone()
        .map(|axis| rect.min().nth_or_panic(axis))
        .collect();
    let max: Vec<f64> = axes.map(|axis| rect.max().nth_or_panic(axis)).collect();
    box_geometry(&min, &max)
}

/// Parses PostGIS's text input: hex-encoded EWKB if `hex` allows it, or else (E)WKT.
fn parse_texts(
    name: &str,
    texts: &arrow_array::StringArray,
    output_type: WkbType,
    column_srid: i32,
    hex: bool,
) -> GeoDataFusionResult<Arc<dyn Array>> {
    let mut builder = WkbBuilder::<i32>::new(output_type);
    for text in texts {
        let Some(text) = text else {
            builder.push_geometry(None::<&wkt::Wkt<f64>>)?;
            continue;
        };
        if hex && is_hex_ewkb(text) {
            let buffer = decode_hex(text).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
            push_ewkb(name, &mut builder, &buffer, column_srid)?;
            continue;
        }
        let (srid, geometry) = parse_ewkt(text).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
        check_row_srid(name, srid, column_srid)?;
        builder.push_geometry(Some(&geometry))?;
    }
    Ok(builder.finish().into_array_ref())
}

fn push_ewkb(
    name: &str,
    builder: &mut WkbBuilder<i32>,
    buffer: &[u8],
    column_srid: i32,
) -> GeoDataFusionResult<()> {
    check_row_srid(name, ewkb_srid(buffer), column_srid)?;
    let geometry = wkb::reader::read_wkb(buffer)
        .map_err(|e| exec_datafusion_err!("{name}: invalid WKB: {e}"))?;
    builder.push_geometry(Some(&geometry))?;
    Ok(())
}

/// A row's embedded SRID must be the column's, or unknown.
fn check_row_srid(name: &str, srid: Option<i32>, column_srid: i32) -> Result<()> {
    match srid {
        Some(srid) if srid != SRID_UNKNOWN && srid != column_srid => Err(exec_datafusion_err!(
            "{name}: Geometry SRID ({srid}) does not match column SRID ({column_srid}); \
             geodatafusion stores one SRID per column"
        )),
        _ => Ok(()),
    }
}

/// PostGIS reads text starting with `0` as hex-encoded EWKB: its first byte, the byte order, is
/// `00` or `01`.
fn is_hex_ewkb(text: &str) -> bool {
    text.starts_with('0')
}

fn decode_hex(text: &str) -> std::result::Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!(
            "Invalid hex string, length ({}) has to be a multiple of two!",
            text.len()
        ));
    }
    let digit = |byte: u8| -> std::result::Result<u8, String> {
        (byte as char)
            .to_digit(16)
            .map(|digit| digit as u8)
            .ok_or_else(|| format!("Invalid hex character {:?}", byte as char))
    };
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}
