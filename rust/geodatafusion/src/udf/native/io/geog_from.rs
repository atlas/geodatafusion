//! Geography input: ST_GeogFromText, ST_GeographyFromText and ST_GeogFromWKB.

use std::sync::LazyLock;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;

use crate::udf::native::types::{Target, cast_impl, cast_return_field};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::SRID_UNKNOWN;

/// PostGIS: ST_GeogFromText(text EWKT) and ST_GeographyFromText(text EWKT).
static TEXT_ARGUMENTS: &[&[Arg]] = &[&[Arg::Text]];

static TEXT_SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// PostGIS: ST_GeogFromWKB(bytea wkb).
static WKB_ARGUMENTS: &[&[Arg]] = &[&[Arg::Bytea]];

static WKB_SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Return a specified geography value from Well-Known Text representation or extended (WKT).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the geography that WKT or EWKT text describes. The SRID is the text's, and 4326 without one; it must be a longitude/latitude SRID. As in PostGIS, coordinates beyond the range of longitude and latitude are brought into it: longitudes by whole turns, latitudes folded back over the poles. An embedded SRID must be the same in every row, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_GeogFromText(EWKT)",
    argument(name = "EWKT", description = "text"),
    related_udf(name = "st_geographyfromtext"),
    related_udf(name = "st_geogfromwkb")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeogFromText;

impl GeogFromText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeogFromText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeogFromText {
    fn name(&self) -> &str {
        "st_geogfromtext"
    }

    fn signature(&self) -> &Signature {
        &TEXT_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cast_return_field(self.name(), &args, SRID_UNKNOWN, Target::Geography)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, TEXT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cast_impl(self.name(), args, Target::Geography, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Return a specified geography value from Well-Known Text representation or extended (WKT).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "An alias for ST_GeogFromText: returns the geography that WKT or EWKT text describes, with SRID 4326 unless the text names another longitude/latitude SRID.",
    syntax_example = "ST_GeographyFromText(EWKT)",
    argument(name = "EWKT", description = "text"),
    related_udf(name = "st_geogfromtext")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeographyFromText;

impl GeographyFromText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeographyFromText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeographyFromText {
    fn name(&self) -> &str {
        "st_geographyfromtext"
    }

    fn signature(&self) -> &Signature {
        &TEXT_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cast_return_field(self.name(), &args, SRID_UNKNOWN, Target::Geography)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, TEXT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cast_impl(self.name(), args, Target::Geography, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Creates a geography instance from a Well-Known Binary geometry representation (WKB) or extended Well Known Binary (EWKB).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Returns the geography that WKB or EWKB describes. The SRID is the EWKB's, and 4326 without one; it must be a longitude/latitude SRID. As in PostGIS, coordinates beyond the range of longitude and latitude are brought into it. An embedded SRID must be the same in every row, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_GeogFromWKB(wkb)",
    argument(name = "wkb", description = "bytea"),
    related_udf(name = "st_geogfromtext")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeogFromWKB;

impl GeogFromWKB {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeogFromWKB {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeogFromWKB {
    fn name(&self) -> &str {
        "st_geogfromwkb"
    }

    fn signature(&self) -> &Signature {
        &WKB_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        cast_return_field(self.name(), &args, SRID_UNKNOWN, Target::Geography)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, WKB_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(cast_impl(self.name(), args, Target::Geography, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}
