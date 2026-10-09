use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_schema::{DataType, FieldRef};
use datafusion::arrow::compute::cast;
use datafusion::common::{ScalarValue, exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, Metadata};
use serde_json::{Map, Value};
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::error::GeoDataFusionResult;
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid, srid_authority_name, srid_to_crs};

/// PostGIS: ST_GeomFromGeoJSON(text geomjson). The `json` and `jsonb` forms have no DataFusion
/// type to take.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geomjson"])
        .expect("parameter names are valid for a user-defined signature")
});

/// The SRID of GeoJSON without a `crs` member, as in PostGIS.
const DEFAULT_SRID: i32 = 4326;

/// Takes as input a geojson representation of a geometry and outputs a PostGIS geometry object.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the geometry of a GeoJSON geometry object. The SRID is 4326, or the one a crs member names (EPSG:n or urn:ogc:def:crs:EPSG::n; 0 if the name is unknown). Because geodatafusion stores one CRS per column, the crs of a non-constant argument must give the SRID of the column, 4326 unless the call is planned with a constant. A Z in any position makes the whole geometry 3D, with Z 0 where it is missing; a fourth ordinate is dropped. Feature objects are an error, as in PostGIS.",
    syntax_example = "ST_GeomFromGeoJSON(geomjson)",
    argument(name = "geomjson", description = "text"),
    related_udf(name = "st_asgeojson")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromGeoJSON;

impl GeomFromGeoJSON {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomFromGeoJSON {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromGeoJSON {
    fn name(&self) -> &str {
        "st_geomfromgeojson"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        geom_from_geojson_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_geojson_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// WKB with the CRS of a constant argument's SRID, or 4326. A constant that doesn't parse gets
/// 4326 here and fails when executed.
fn geom_from_geojson_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let literal = match args.scalar_arguments.first() {
        Some(Some(ScalarValue::Utf8(Some(text))))
        | Some(Some(ScalarValue::LargeUtf8(Some(text))))
        | Some(Some(ScalarValue::Utf8View(Some(text)))) => Some(text.as_str()),
        _ => None,
    };
    let srid = literal
        .and_then(|text| parse_geojson(text).ok())
        .map_or(DEFAULT_SRID, |(srid, _)| srid);
    Ok(wkb_return_field(
        name,
        Arc::new(Metadata::new(srid_to_crs(srid), None)),
    ))
}

fn geom_from_geojson_impl(
    name: &str,
    args: ScalarFunctionArgs,
) -> GeoDataFusionResult<ColumnarValue> {
    let column_srid = crs_to_srid(input_metadata(&args.return_field).crs()).unwrap_or(SRID_UNKNOWN);
    let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(exec_datafusion_err!("{name}: expected a WKB return field").into());
    };
    let text = cast(&args.args[0].to_array(args.number_rows)?, &DataType::Utf8)?;
    let mut builder = WkbBuilder::<i32>::new(wkb_type);
    for value in text.as_string::<i32>() {
        let Some(value) = value else {
            builder.push_geometry(None::<&Wkt<f64>>)?;
            continue;
        };
        let (srid, geometry) =
            parse_geojson(value).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
        if srid != column_srid {
            return Err(exec_datafusion_err!(
                "{name}: GeoJSON with SRID {srid} in a column of SRID {column_srid}; geodatafusion stores one SRID per column"
            )
            .into());
        }
        builder.push_geometry(Some(&geometry))?;
    }
    Ok(ColumnarValue::Array(builder.finish().to_array_ref()))
}

/// Parses a GeoJSON geometry object the way PostGIS does, returning its SRID and geometry.
fn parse_geojson(text: &str) -> std::result::Result<(i32, Wkt<f64>), String> {
    // PostGIS's JSON parser reads the first value and ignores what follows.
    let value = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .next()
        .ok_or("unexpected end of data")?
        .map_err(|e| e.to_string())?;
    let Value::Object(object) = &value else {
        return Err("unknown GeoJSON type".to_string());
    };
    let srid = crs_srid(object);
    let parsed = parse_object(object)?;
    let dim = if parsed.has_z() {
        Dimension::XYZ
    } else {
        Dimension::XY
    };
    Ok((srid, parsed.into_wkt(dim)))
}

/// The SRID a `crs` member names: 4326 without one (or one that isn't of type `name`), and 0
/// for a name PostGIS doesn't know.
fn crs_srid(object: &Map<String, Value>) -> i32 {
    let Some(name) = object
        .get("crs")
        .and_then(|crs| crs.get("properties"))
        .and_then(|properties| properties.get("name"))
        .and_then(Value::as_str)
    else {
        return DEFAULT_SRID;
    };
    srid_from_name(name).unwrap_or(SRID_UNKNOWN)
}

/// The SRID of a CRS name: `AUTHORITY:code` or `urn:ogc:def:crs:AUTHORITY:[version]:code`, with
/// an authority that defines the code in PostGIS, in any case.
fn srid_from_name(name: &str) -> Option<i32> {
    const URN_PREFIX: &str = "urn:ogc:def:crs:";
    let name = match name.get(..URN_PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(URN_PREFIX) => &name[URN_PREFIX.len()..],
        _ => name,
    };
    let (authority, _) = name.split_once(':')?;
    let (_, code) = name.rsplit_once(':')?;
    let srid: i32 = code.parse().ok()?;
    srid_authority_name(srid)
        .filter(|known| known.eq_ignore_ascii_case(authority))
        .map(|_| srid)
}

/// A geometry as parsed, positions holding 2 or 3 ordinates, before the dimension of the whole
/// geometry is known.
enum Parsed {
    Point(Option<Vec<f64>>),
    LineString(Vec<Vec<f64>>),
    Polygon(Vec<Vec<Vec<f64>>>),
    MultiPoint(Vec<Vec<f64>>),
    MultiLineString(Vec<Vec<Vec<f64>>>),
    MultiPolygon(Vec<Vec<Vec<Vec<f64>>>>),
    GeometryCollection(Vec<Parsed>),
}

impl Parsed {
    fn has_z(&self) -> bool {
        let line_has_z = |line: &Vec<Vec<f64>>| line.iter().any(|position| position.len() > 2);
        match self {
            Parsed::Point(position) => position.as_ref().is_some_and(|p| p.len() > 2),
            Parsed::LineString(line) | Parsed::MultiPoint(line) => line_has_z(line),
            Parsed::Polygon(rings) | Parsed::MultiLineString(rings) => rings.iter().any(line_has_z),
            Parsed::MultiPolygon(polygons) => {
                polygons.iter().any(|rings| rings.iter().any(line_has_z))
            }
            Parsed::GeometryCollection(members) => members.iter().any(Parsed::has_z),
        }
    }

    fn into_wkt(self, dim: Dimension) -> Wkt<f64> {
        // `position` keeps 2 or 3 ordinates.
        let coord = |position: Vec<f64>| Coord {
            x: position[0],
            y: position[1],
            z: (dim == Dimension::XYZ).then(|| position.get(2).copied().unwrap_or(0.0)),
            m: None,
        };
        let line = |positions: Vec<Vec<f64>>| {
            LineString::new(positions.into_iter().map(coord).collect(), dim)
        };
        let polygon =
            |rings: Vec<Vec<Vec<f64>>>| Polygon::new(rings.into_iter().map(line).collect(), dim);
        match self {
            Parsed::Point(position) => Wkt::Point(Point::new(position.map(coord), dim)),
            Parsed::LineString(positions) => Wkt::LineString(line(positions)),
            Parsed::Polygon(rings) => Wkt::Polygon(polygon(rings)),
            Parsed::MultiPoint(positions) => Wkt::MultiPoint(MultiPoint::new(
                positions
                    .into_iter()
                    .map(|position| Point::new(Some(coord(position)), dim))
                    .collect(),
                dim,
            )),
            Parsed::MultiLineString(lines) => Wkt::MultiLineString(MultiLineString::new(
                lines.into_iter().map(line).collect(),
                dim,
            )),
            Parsed::MultiPolygon(polygons) => Wkt::MultiPolygon(MultiPolygon::new(
                polygons.into_iter().map(polygon).collect(),
                dim,
            )),
            Parsed::GeometryCollection(members) => {
                Wkt::GeometryCollection(GeometryCollection::new(
                    members
                        .into_iter()
                        .map(|member| member.into_wkt(dim))
                        .collect(),
                    dim,
                ))
            }
        }
    }
}

const INVALID: &str = "invalid GeoJson representation";
const NOT_NESTED: &str = "The 'coordinates' in GeoJSON are not sufficiently nested";

fn parse_object(object: &Map<String, Value>) -> std::result::Result<Parsed, String> {
    let Some(type_name) = object.get("type") else {
        return Err("unknown GeoJSON type".to_string());
    };
    let Some(type_name) = type_name.as_str() else {
        return Err(INVALID.to_string());
    };
    let type_name = type_name.to_ascii_lowercase();
    if type_name == "geometrycollection" {
        let Some(geometries) = object.get("geometries").filter(|g| !g.is_null()) else {
            return Err("Unable to find 'geometries' in GeoJSON string".to_string());
        };
        let members = match geometries {
            Value::Array(members) => members
                .iter()
                .map(|member| match member {
                    Value::Object(member) => parse_object(member),
                    _ => Err(INVALID.to_string()),
                })
                .collect::<std::result::Result<_, _>>()?,
            _ => vec![],
        };
        return Ok(Parsed::GeometryCollection(members));
    }
    let parse: fn(&[Value]) -> std::result::Result<Parsed, String> = match type_name.as_str() {
        "point" => |c| Ok(Parsed::Point(position(c)?)),
        "linestring" => |c| Ok(Parsed::LineString(positions(c)?)),
        "polygon" => |c| Ok(Parsed::Polygon(rings(c)?)),
        "multipoint" => |c| Ok(Parsed::MultiPoint(positions(c)?)),
        "multilinestring" => |c| {
            let lines = arrays(c)?
                .into_iter()
                .map(positions)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(Parsed::MultiLineString(
                lines.into_iter().filter(|line| !line.is_empty()).collect(),
            ))
        },
        "multipolygon" => |c| {
            let polygons = arrays(c)?
                .into_iter()
                .map(rings)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(Parsed::MultiPolygon(
                polygons
                    .into_iter()
                    .filter(|polygon| !polygon.is_empty())
                    .collect(),
            ))
        },
        _ => return Err(INVALID.to_string()),
    };
    let coordinates = match object.get("coordinates") {
        None | Some(Value::Null) => {
            return Err("Unable to find 'coordinates' in GeoJSON string".to_string());
        }
        Some(Value::Array(coordinates)) => coordinates,
        Some(_) => return Err("The 'coordinates' in GeoJSON are not an array".to_string()),
    };
    parse(coordinates)
}

/// The elements of an array that must hold arrays.
fn arrays(values: &[Value]) -> std::result::Result<Vec<&[Value]>, String> {
    values
        .iter()
        .map(|value| match value {
            Value::Array(items) => Ok(items.as_slice()),
            _ => Err(NOT_NESTED.to_string()),
        })
        .collect()
}

/// A position: `None` for `[]`, an error for one ordinate, and x, y and Z (a fourth ordinate is
/// dropped). Non-numbers read as PostGIS's JSON library reads them: numeric strings parse, true
/// is 1, anything else 0.
fn position(values: &[Value]) -> std::result::Result<Option<Vec<f64>>, String> {
    match values.len() {
        0 => Ok(None),
        1 => Err("Too few ordinates in GeoJSON".to_string()),
        n => Ok(Some(values[..n.min(3)].iter().map(ordinate).collect())),
    }
}

fn ordinate(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::String(text) => text.trim().parse().unwrap_or(0.0),
        Value::Bool(true) => 1.0,
        _ => 0.0,
    }
}

/// The positions of a line or multipoint; empty positions are skipped.
fn positions(values: &[Value]) -> std::result::Result<Vec<Vec<f64>>, String> {
    Ok(arrays(values)?
        .into_iter()
        .map(position)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// The rings of a polygon. An empty exterior ring makes the polygon empty; empty interior rings
/// are dropped.
fn rings(values: &[Value]) -> std::result::Result<Vec<Vec<Vec<f64>>>, String> {
    let rings = arrays(values)?
        .into_iter()
        .map(positions)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rings.first().is_none_or(Vec::is_empty) {
        return Ok(vec![]);
    }
    Ok(rings.into_iter().filter(|ring| !ring.is_empty()).collect())
}
