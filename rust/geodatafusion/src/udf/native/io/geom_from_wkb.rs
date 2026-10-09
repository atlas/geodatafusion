//! Constructors from Well-Known Binary: ST_GeomFromWKB, ST_GeomFromEWKB and the type-checked
//! ST_PointFromWKB family, which return NULL when the WKB holds another geometry type.

use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_array::new_null_array;
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{ScalarValue, exec_datafusion_err, internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, Metadata};

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::expected_type::ExpectedType;
use crate::udf::native::io::util::wkb::ewkb_srid;
use crate::util::args::scalar_srid;
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid, srid_to_crs};

/// PostGIS: ST_GeomFromWKB(bytea geom) and ST_GeomFromWKB(bytea geom, integer srid). PostGIS
/// doesn't name the parameters.
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Bytea], &[Arg::Bytea, Arg::Srid]];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Returns a geometry from Well-Known Binary (WKB).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a geometry from the OGC Well-Known Binary representation, in either byte order. EWKB is accepted too, and its SRID kept; the srid argument overrides it. Unlike PostGIS, an embedded SRID must be the same in every row, because geodatafusion stores one CRS per column, and curves, triangles and nested collections are not supported.",
    syntax_example = "ST_GeomFromWKB(geom, srid)",
    argument(name = "geom", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_asbinary"),
    related_udf(name = "st_geomfromewkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromWKB {
    aliases: Vec<String>,
}

impl GeomFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_wkbtosql".to_string()],
        }
    }
}

impl Default for GeomFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromWKB {
    fn name(&self) -> &str {
        "st_geomfromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(self.name(), args, None)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: ST_GeomFromEWKB(bytea EWKB).
static EWKB_ARGUMENTS: &[&[Arg]] = &[&[Arg::Bytea]];

/// PostGIS keeps the name without `ST_`.
static GEOMFROMEWKB_ALIASES: LazyLock<Vec<String>> =
    LazyLock::new(|| vec!["geomfromewkb".to_string()]);

/// Returns a geometry from Extended Well-Known Binary (EWKB).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a geometry from the Extended Well-Known Binary representation, in either byte order, keeping its SRID. GeomFromEWKB is an alias. ISO WKB is accepted too. Unlike PostGIS, the SRID must be the same in every row, because geodatafusion stores one CRS per column, and curves, triangles and nested collections are not supported.",
    syntax_example = "ST_GeomFromEWKB(EWKB)",
    argument(name = "EWKB", description = "bytea"),
    related_udf(name = "st_asewkb"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromEWKB;

impl GeomFromEWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomFromEWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromEWKB {
    fn name(&self) -> &str {
        "st_geomfromewkb"
    }

    fn aliases(&self) -> &[String] {
        &GEOMFROMEWKB_ALIASES
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, EWKB_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(self.name(), args, None)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a POINT from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a POINT from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_PointFromWKB(geom, srid)",
    argument(name = "geom", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointFromWKB;

impl PointFromWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointFromWKB {
    fn name(&self) -> &str {
        "st_pointfromwkb"
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::Point),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a LINESTRING from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a LINESTRING from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_LineFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct LineFromWKB {
    aliases: Vec<String>,
}

impl LineFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_linestringfromwkb".to_string()],
        }
    }
}

impl Default for LineFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for LineFromWKB {
    fn name(&self) -> &str {
        "st_linefromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::LineString),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a POLYGON from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a POLYGON from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_PolyFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PolyFromWKB {
    aliases: Vec<String>,
}

impl PolyFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_polygonfromwkb".to_string()],
        }
    }
}

impl Default for PolyFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PolyFromWKB {
    fn name(&self) -> &str {
        "st_polyfromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::Polygon),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a MULTIPOINT from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a MULTIPOINT from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_MPointFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MPointFromWKB {
    aliases: Vec<String>,
}

impl MPointFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_multipointfromwkb".to_string()],
        }
    }
}

impl Default for MPointFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MPointFromWKB {
    fn name(&self) -> &str {
        "st_mpointfromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::MultiPoint),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a MULTILINESTRING from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a MULTILINESTRING from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_MLineFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MLineFromWKB {
    aliases: Vec<String>,
}

impl MLineFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_multilinefromwkb".to_string()],
        }
    }
}

impl Default for MLineFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MLineFromWKB {
    fn name(&self) -> &str {
        "st_mlinefromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::MultiLineString),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a MULTIPOLYGON from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a MULTIPOLYGON from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_MPolyFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MPolyFromWKB {
    aliases: Vec<String>,
}

impl MPolyFromWKB {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_multipolyfromwkb".to_string()],
        }
    }
}

impl Default for MPolyFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MPolyFromWKB {
    fn name(&self) -> &str {
        "st_mpolyfromwkb"
    }

    fn aliases(&self) -> &[String] {
        &self.aliases
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::MultiPolygon),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Returns a GEOMETRYCOLLECTION from Well-Known Binary (WKB), or NULL for another geometry type.
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a GEOMETRYCOLLECTION from the OGC Well-Known Binary representation, or returns NULL if the WKB holds another geometry type. EWKB is accepted, and the srid argument overrides an embedded SRID.",
    syntax_example = "ST_GeomCollFromWKB(WKB, srid)",
    argument(name = "WKB", description = "bytea"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_geomfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomCollFromWKB;

impl GeomCollFromWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomCollFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomCollFromWKB {
    fn name(&self) -> &str {
        "st_geomcollfromwkb"
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
            output_metadata(self.name(), &args)?,
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_wkb_impl(
            self.name(),
            args,
            Some(ExpectedType::GeometryCollection),
        )?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The metadata of the output column: the CRS of the `srid` argument, else of a literal
/// argument's EWKB SRID, else the input field's (a `geoarrow.wkb` column keeps its CRS).
fn output_metadata(name: &str, args: &ReturnFieldArgs) -> Result<Arc<Metadata>> {
    let srid = if args.arg_fields.len() > 1 {
        Some(scalar_srid(name, args, 1)?.unwrap_or(SRID_UNKNOWN))
    } else {
        match args.scalar_arguments.first() {
            Some(Some(
                ScalarValue::Binary(Some(bytes))
                | ScalarValue::LargeBinary(Some(bytes))
                | ScalarValue::BinaryView(Some(bytes)),
            )) => ewkb_srid(bytes),
            _ => None,
        }
    };
    Ok(match srid {
        Some(srid) => Arc::new(Metadata::new(srid_to_crs(srid), None)),
        None => input_metadata(&args.arg_fields[0]),
    })
}

/// Parses the (E)WKB of every row and writes it as little-endian ISO WKB.
///
/// An optional `srid` argument overrides an embedded SRID, as in PostGIS. Otherwise every
/// row's EWKB SRID must agree with the output column's.
/// A type-checked constructor passes the type it accepts and gets NULL for any other type.
fn geom_from_wkb_impl(
    name: &str,
    args: ScalarFunctionArgs,
    expected: Option<ExpectedType>,
) -> GeoDataFusionResult<ColumnarValue> {
    // SQL NULL in, SQL NULL out.
    if matches!(args.args.get(1), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let srid_argument_given = args.args.len() > 1;
    let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(internal_datafusion_err!("{name}: unexpected return field").into());
    };
    let column_srid = crs_to_srid(output_type.metadata().crs()).unwrap_or(SRID_UNKNOWN);
    let buffers = args.args[0]
        .cast_to(&DataType::Binary, None)?
        .to_array(args.number_rows)?;

    let mut builder = WkbBuilder::<i32>::new(output_type);
    for buffer in buffers.as_binary::<i32>() {
        let Some(buffer) = buffer else {
            builder.push_geometry(None::<&wkt::Wkt<f64>>)?;
            continue;
        };
        if !srid_argument_given
            && let Some(srid) = ewkb_srid(buffer).filter(|srid| *srid != column_srid)
        {
            return Err(exec_datafusion_err!(
                "{name}: Geometry SRID ({srid}) does not match column SRID ({column_srid}); \
                 geodatafusion stores one SRID per column"
            )
            .into());
        }
        let geometry = wkb::reader::read_wkb(buffer)
            .map_err(|e| exec_datafusion_err!("{name}: invalid WKB: {e}"))?;
        let accepted = expected.is_none_or(|expected| expected.matches(&geometry));
        builder.push_geometry(accepted.then_some(&geometry))?;
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}
