//! Constructors from Well-Known Text: ST_GeomFromText and ST_GeomFromEWKT.

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
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::native::io::util::wkt::{ewkt_srid_prefix, parse_ewkt};
use crate::util::args::scalar_srid;
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid, srid_to_crs};

/// PostGIS: ST_GeomFromText(text WKT) and ST_GeomFromText(text WKT, integer srid).
static ARGUMENTS: &[&[Arg]] = &[&[Arg::Text], &[Arg::Text, Arg::Srid]];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["WKT", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a geometry from Well-Known Text (WKT).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a geometry from the OGC Well-Known Text representation, accepting what PostGIS accepts: implicit dimensions (POINT(1 2 3) is POINT Z), an SRID=n; prefix, and EMPTY members. The srid argument overrides an embedded SRID. Unlike PostGIS, an embedded SRID must be the same in every row, because geodatafusion stores one CRS per column, and curves, triangles and nested collections are not supported.",
    syntax_example = "ST_GeomFromText(WKT, srid)",
    argument(name = "WKT", description = "text"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_astext"),
    related_udf(name = "st_geomfromewkt")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromText {
    aliases: Vec<String>,
}

impl GeomFromText {
    pub fn new() -> Self {
        Self {
            aliases: vec!["st_geometryfromtext".to_string(), "st_wkttosql".to_string()],
        }
    }
}

impl Default for GeomFromText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromText {
    fn name(&self) -> &str {
        "st_geomfromtext"
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
        let srid = output_srid(self.name(), &args)?;
        let metadata = Arc::new(Metadata::new(srid_to_crs(srid), None));
        Ok(wkb_return_field(self.name(), metadata))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_text_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: ST_GeomFromEWKT(text EWKT).
static EWKT_ARGUMENTS: &[&[Arg]] = &[&[Arg::Text]];

static EWKT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["EWKT"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Returns a geometry from Extended Well-Known Text (EWKT).
#[user_doc(
    doc_section(label = "Geometry Input"),
    description = "Constructs a geometry from the Extended Well-Known Text representation: WKT with an optional SRID=n; prefix. Unlike PostGIS, the SRID must be the same in every row, because geodatafusion stores one CRS per column, and curves, triangles and nested collections are not supported.",
    syntax_example = "ST_GeomFromEWKT(EWKT)",
    argument(name = "EWKT", description = "text"),
    related_udf(name = "st_asewkt"),
    related_udf(name = "st_geomfromtext")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct GeomFromEWKT;

impl GeomFromEWKT {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeomFromEWKT {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for GeomFromEWKT {
    fn name(&self) -> &str {
        "st_geomfromewkt"
    }

    fn signature(&self) -> &Signature {
        &EWKT_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let srid = output_srid(self.name(), &args)?;
        let metadata = Arc::new(Metadata::new(srid_to_crs(srid), None));
        Ok(wkb_return_field(self.name(), metadata))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, EWKT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(geom_from_ewkt_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The SRID of the output column: the `srid` argument, else the `SRID=n;` prefix of a literal
/// WKT argument, else unknown.
fn output_srid(name: &str, args: &ReturnFieldArgs) -> Result<i32> {
    if args.arg_fields.len() > 1 {
        return Ok(scalar_srid(name, args, 1)?.unwrap_or(SRID_UNKNOWN));
    }
    let literal = match args.scalar_arguments.first() {
        Some(Some(
            ScalarValue::Utf8(Some(text))
            | ScalarValue::LargeUtf8(Some(text))
            | ScalarValue::Utf8View(Some(text)),
        )) => Some(text.as_str()),
        _ => None,
    };
    Ok(literal.and_then(ewkt_srid_prefix).unwrap_or(SRID_UNKNOWN))
}

fn geom_from_text_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    // SQL NULL in, SQL NULL out.
    if matches!(args.args.get(1), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    geom_from_ewkt_impl(name, args)
}

fn geom_from_ewkt_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = parse_rows(name, &args)?;
    let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(&args.return_field)? else {
        return Err(internal_datafusion_err!("{name}: unexpected return field").into());
    };
    let mut builder = WkbBuilder::<i32>::new(output_type);
    for geometry in &geometries {
        builder.push_geometry(geometry.as_ref())?;
    }
    Ok(ColumnarValue::Array(builder.finish().into_array_ref()))
}

/// Parses the text argument of every row.
///
/// An optional `srid` argument overrides an embedded SRID, as in PostGIS. Otherwise the output
/// column's CRS came from a literal's prefix when planning, and every row must agree with it.
fn parse_rows(name: &str, args: &ScalarFunctionArgs) -> Result<Vec<Option<Wkt<f64>>>> {
    let srid_argument_given = args.args.len() > 1;
    let column_srid = crs_to_srid(input_metadata(&args.return_field).crs()).unwrap_or(SRID_UNKNOWN);
    let texts = args.args[0]
        .cast_to(&DataType::Utf8, None)?
        .to_array(args.number_rows)?;
    texts
        .as_string::<i32>()
        .iter()
        .map(|text| {
            let Some(text) = text else {
                return Ok(None);
            };
            let (srid, geometry) =
                parse_ewkt(text).map_err(|e| exec_datafusion_err!("{name}: {e}"))?;
            if !srid_argument_given && let Some(srid) = srid.filter(|srid| *srid != column_srid) {
                return Err(exec_datafusion_err!(
                    "{name}: Geometry SRID ({srid}) does not match column SRID ({column_srid}); \
                     geodatafusion stores one SRID per column"
                ));
            }
            Ok(Some(geometry))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;
    use geoarrow_schema::WkbType;
    use geoarrow_schema::crs::Crs;

    use super::*;

    #[tokio::test]
    async fn test_srid_sets_crs() {
        let ctx = SessionContext::new();
        ctx.register_udf(GeomFromText::default().into());

        for sql in [
            "SELECT ST_GeomFromText('POINT(1 2)', 4326)",
            "SELECT ST_GeomFromText('SRID=4326;POINT(1 2)')",
        ] {
            let batch = ctx.sql(sql).await.unwrap().collect().await.unwrap();
            let field = batch[0].schema().field(0).clone();
            let output_type = field.try_extension_type::<WkbType>().unwrap();
            assert_eq!(
                output_type.metadata().crs(),
                &Crs::from_authority_code("EPSG:4326".to_string()),
                "{sql}"
            );
        }
    }
}
