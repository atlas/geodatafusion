use std::sync::{Arc, LazyLock};

use arrow_array::{Array, Int32Array, StringArray};
use arrow_schema::DataType;
use datafusion::common::exec_datafusion_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ScalarFunctionArgs, ScalarUDFImpl, Signature, Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::io::util::number::{write_fixed, write_number};
use crate::util::args::optional_int_arg;
use crate::util::field::{geometry_array, input_metadata};
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::z;
use crate::util::owned::to_owned_geometry;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid, srid_authority_name};

/// PostGIS: ST_AsGeoJSON(geometry geom, integer maxdecimaldigits = 9, integer options = 8).
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry],
    &[Arg::Geometry, Arg::Integer],
    &[Arg::Geometry, Arg::Integer, Arg::Integer],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "maxdecimaldigits", "options"])
        .expect("parameter names are valid for a user-defined signature")
});

/// PostGIS's defaults for ST_AsGeoJSON.
const DEFAULT_MAX_DECIMAL_DIGITS: i32 = 9;
const DEFAULT_OPTIONS: i32 = 8;

/// The `options` bits.
const OPTION_BBOX: i32 = 1;
const OPTION_SHORT_CRS: i32 = 2;
const OPTION_LONG_CRS: i32 = 4;
const OPTION_SHORT_CRS_UNLESS_4326: i32 = 8;

/// Return a geometry as a GeoJSON element.
#[user_doc(
    doc_section(label = "Geometry Output"),
    description = "Returns the geometry as a GeoJSON geometry object, with coordinates written with at most maxdecimaldigits decimals (default 9) and without M. options is a bitmask: 1 adds a bbox, 2 a short CRS name (EPSG:4326), 4 a long one (urn:ogc:def:crs:EPSG::4326), and 8, the default, a short one unless the SRID is 4326. A geometry without an SRID gets no CRS. Nested GEOMETRYCOLLECTIONs are an error, as in PostGIS. The record form of PostGIS's ST_AsGeoJSON isn't supported.",
    syntax_example = "ST_AsGeoJSON(geom, maxdecimaldigits, options)",
    argument(name = "geom", description = "geometry"),
    argument(name = "maxdecimaldigits", description = "integer, default 9"),
    argument(name = "options", description = "integer, default 8"),
    related_udf(name = "st_geomfromgeojson")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct AsGeoJSON;

impl AsGeoJSON {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsGeoJSON {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for AsGeoJSON {
    fn name(&self) -> &str {
        "st_asgeojson"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(as_geojson_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn as_geojson_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let metadata = input_metadata(&args.arg_fields[0]);
    let kernel = AsGeoJSONKernel {
        srid: crs_to_srid(metadata.crs()).unwrap_or(SRID_UNKNOWN),
        max_decimal_digits: optional_int_arg(&args, 1, DEFAULT_MAX_DECIMAL_DIGITS)?,
        options: optional_int_arg(&args, 2, DEFAULT_OPTIONS)?,
    };
    let result: StringArray = map_geometry(geometries.as_ref(), &kernel)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct AsGeoJSONKernel {
    /// The column's SRID; one per column.
    srid: i32,
    max_decimal_digits: Int32Array,
    options: Int32Array,
}

impl AsGeoJSONKernel {
    /// The `crs` member's name, if `options` asks for one.
    fn crs_name(&self, options: i32) -> GeoDataFusionResult<Option<String>> {
        if self.srid == SRID_UNKNOWN {
            return Ok(None);
        }
        let long = options & OPTION_LONG_CRS != 0;
        let wanted = long
            || options & OPTION_SHORT_CRS != 0
            || (options & OPTION_SHORT_CRS_UNLESS_4326 != 0 && self.srid != 4326);
        if !wanted {
            return Ok(None);
        }
        let Some(authority) = srid_authority_name(self.srid) else {
            return Err(exec_datafusion_err!(
                "st_asgeojson: SRID {} unknown in spatial_ref_sys table",
                self.srid
            )
            .into());
        };
        Ok(Some(if long {
            format!("urn:ogc:def:crs:{authority}::{}", self.srid)
        } else {
            format!("{authority}:{}", self.srid)
        }))
    }
}

impl GeometryKernel for AsGeoJSONKernel {
    type Output = String;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<String>> {
        // SQL NULL in any argument, SQL NULL out.
        if self.max_decimal_digits.is_null(row) || self.options.is_null(row) {
            return Ok(None);
        }
        let options = self.options.value(row);
        let writer = GeoJsonWriter {
            decimals: self.max_decimal_digits.value(row),
            with_z: matches!(geom.dim(), Dimensions::Xyz | Dimensions::Xyzm),
        };
        let mut out = String::new();
        let crs = self.crs_name(options)?;
        writer.write_geometry(
            &mut out,
            geom,
            crs.as_deref(),
            options & OPTION_BBOX != 0,
            true,
        )?;
        Ok(Some(out))
    }
}

struct GeoJsonWriter {
    decimals: i32,
    /// Whether to write Z: GeoJSON positions have no M.
    with_z: bool,
}

impl GeoJsonWriter {
    /// Writes one geometry object: `{"type":...,"crs":...,"bbox":...,"coordinates":...}`. Only
    /// the top-level object gets a crs and bbox, and only it may be a GEOMETRYCOLLECTION.
    fn write_geometry(
        &self,
        out: &mut String,
        geom: &impl GeometryTrait<T = f64>,
        crs: Option<&str>,
        bbox: bool,
        top_level: bool,
    ) -> GeoDataFusionResult<()> {
        let type_name = match geom.as_type() {
            GeometryType::Point(_) => "Point",
            GeometryType::LineString(_) | GeometryType::Line(_) => "LineString",
            GeometryType::Polygon(_) | GeometryType::Rect(_) | GeometryType::Triangle(_) => {
                "Polygon"
            }
            GeometryType::MultiPoint(_) => "MultiPoint",
            GeometryType::MultiLineString(_) => "MultiLineString",
            GeometryType::MultiPolygon(_) => "MultiPolygon",
            GeometryType::GeometryCollection(_) if top_level => "GeometryCollection",
            GeometryType::GeometryCollection(_) => {
                return Err(
                    exec_datafusion_err!("st_asgeojson: GeoJson: geometry not supported.").into(),
                );
            }
        };
        out.push_str(r#"{"type":""#);
        out.push_str(type_name);
        out.push('"');
        if let Some(crs) = crs {
            out.push_str(r#","crs":{"type":"name","properties":{"name":""#);
            out.push_str(crs);
            out.push_str(r#""}}"#);
        }
        if bbox {
            self.write_bbox(out, geom);
        }
        match geom.as_type() {
            GeometryType::GeometryCollection(collection) => {
                out.push_str(r#","geometries":["#);
                for (index, member) in collection.geometries().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    self.write_geometry(out, &member, None, false, false)?;
                }
                out.push(']');
            }
            GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {
                out.push_str(r#","coordinates":"#);
                self.write_coordinates(out, &to_owned_geometry(geom));
            }
            _ => {
                out.push_str(r#","coordinates":"#);
                self.write_coordinates(out, geom);
            }
        }
        out.push('}');
        Ok(())
    }

    /// `"bbox":[minx,miny,(minz,)maxx,maxy(,maxz)]`, written with fixed decimals. PostGIS writes
    /// zeros for an empty geometry.
    fn write_bbox(&self, out: &mut String, geom: &impl GeometryTrait<T = f64>) {
        let mut rect = BoundingRect::new(false);
        rect.add_geometry(geom);
        let values = if rect.is_empty() {
            vec![0.0; if self.with_z { 6 } else { 4 }]
        } else {
            let (minz, maxz) = rect.z_range().unwrap_or((0.0, 0.0));
            let mut values = vec![rect.minx(), rect.miny()];
            if self.with_z {
                values.push(minz);
            }
            values.extend([rect.maxx(), rect.maxy()]);
            if self.with_z {
                values.push(maxz);
            }
            values
        };
        out.push_str(r#","bbox":["#);
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_fixed(out, value, self.decimals);
        }
        out.push(']');
    }

    /// The `coordinates` array of a non-collection geometry.
    fn write_coordinates(&self, out: &mut String, geom: &impl GeometryTrait<T = f64>) {
        match geom.as_type() {
            GeometryType::Point(point) => self.write_point(out, point),
            GeometryType::LineString(line) => self.write_line_string(out, line),
            GeometryType::Polygon(polygon) => self.write_polygon(out, polygon),
            GeometryType::MultiPoint(points) => {
                self.write_array(out, points.points(), |out, point| {
                    self.write_point(out, &point)
                })
            }
            GeometryType::MultiLineString(lines) => {
                self.write_array(out, lines.line_strings(), |out, line| {
                    self.write_line_string(out, &line)
                })
            }
            GeometryType::MultiPolygon(polygons) => {
                self.write_array(out, polygons.polygons(), |out, polygon| {
                    self.write_polygon(out, &polygon)
                })
            }
            // Collections are written as geometries, and the other types converted first.
            _ => out.push_str("[]"),
        }
    }

    fn write_array<I>(
        &self,
        out: &mut String,
        items: I,
        mut write: impl FnMut(&mut String, I::Item),
    ) where
        I: Iterator,
    {
        out.push('[');
        for (index, item) in items.enumerate() {
            if index > 0 {
                out.push(',');
            }
            write(out, item);
        }
        out.push(']');
    }

    /// `[x,y(,z)]`, or `[]` for an empty point.
    fn write_point(&self, out: &mut String, point: &impl PointTrait<T = f64>) {
        match point.coord() {
            Some(coord) => self.write_position(out, &coord),
            None => out.push_str("[]"),
        }
    }

    fn write_line_string(&self, out: &mut String, line: &impl LineStringTrait<T = f64>) {
        self.write_array(out, line.coords(), |out, coord| {
            self.write_position(out, &coord)
        });
    }

    fn write_polygon(&self, out: &mut String, polygon: &impl PolygonTrait<T = f64>) {
        let rings = polygon
            .exterior()
            .filter(|ring| ring.num_coords() > 0)
            .into_iter()
            .chain(polygon.interiors());
        self.write_array(out, rings, |out, ring| self.write_line_string(out, &ring));
    }

    fn write_position(&self, out: &mut String, coord: &impl CoordTrait<T = f64>) {
        out.push('[');
        write_number(out, coord.x(), self.decimals);
        out.push(',');
        write_number(out, coord.y(), self.decimals);
        if self.with_z {
            out.push(',');
            write_number(out, z(coord).unwrap_or(0.0), self.decimals);
        }
        out.push(']');
    }
}
