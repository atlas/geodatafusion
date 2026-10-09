use std::cell::RefCell;
use std::sync::{Arc, LazyLock};

use arrow_array::{Array, StringArray, new_null_array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err, plan_err};
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_schema::Metadata;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::proj::util::crs::proj_definition;
use crate::udf::proj::util::operation::{Definition, with_operation};
use crate::util::args::{optional_text_arg, scalar_srid};
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{map_coords, to_owned_geometry};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid, srid_to_crs};

/// PostGIS:
/// - ST_Transform(geometry g1, integer srid)
/// - ST_Transform(geometry geom, text to_proj)
/// - ST_Transform(geometry geom, text from_proj, text to_proj)
/// - ST_Transform(geometry geom, text from_proj, integer to_srid)
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Srid],
    &[Arg::Geometry, Arg::Text],
    &[Arg::Geometry, Arg::Text, Arg::Text],
    &[Arg::Geometry, Arg::Text, Arg::Srid],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Return a new geometry with coordinates transformed to a different spatial reference system.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Returns the geometry with its coordinates transformed with PROJ: from its SRID, or from from_proj, to the SRID srid (or to_srid), or to to_proj. A CRS can be anything PROJ reads, such as EPSG:4326, a PROJ string or WKT. Z is transformed too and M kept. The result has the target SRID, or SRID 0 when the target is to_proj. A geometry already in the target SRID is returned as it is. Because geodatafusion stores one CRS per column, a target SRID must be a constant. Requires the proj feature.",
    syntax_example = "ST_Transform(g1, srid)",
    alternative_syntax = "ST_Transform(geom, from_proj, to_proj)",
    argument(name = "g1", description = "geometry"),
    argument(name = "srid", description = "integer, or to_proj or from_proj: text"),
    argument(name = "to_proj", description = "text, or to_srid: integer"),
    related_udf(name = "st_setsrid"),
    related_udf(name = "st_transformpipeline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Transform;

impl Transform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Transform {
    fn name(&self) -> &str {
        "st_transform"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        transform_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(transform_impl(self.name(), args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The argument holding the target SRID, if the overload has one.
fn target_srid_index(arg_types: &[DataType]) -> Option<usize> {
    let last = arg_types.len().checked_sub(1)?;
    let is_srid = arg_types[last].is_integer() || arg_types[last].is_null();
    (last > 0 && is_srid).then_some(last)
}

/// WKB with the target SRID's CRS, or no CRS (SRID 0) when the target is a PROJ text.
fn transform_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let types: Vec<DataType> = args
        .arg_fields
        .iter()
        .map(|field| field.data_type().clone())
        .collect();
    let Some(index) = target_srid_index(&types) else {
        return Ok(wkb_return_field(name, Default::default()));
    };
    let crs = match scalar_srid(name, args, index)? {
        None => Default::default(),
        Some(SRID_UNKNOWN) => return plan_err!("{name}: 0 is an invalid target SRID"),
        Some(srid) => srid_to_crs(srid),
    };
    Ok(wkb_return_field(name, Arc::new(Metadata::new(crs, None))))
}

/// Where a row's coordinates come from and go to.
enum Target {
    /// The column's CRS (or `from_proj`) to the return field's.
    Srid { to: String, to_srid: i32 },
    /// To a PROJ text, per row.
    Proj(StringArray),
}

fn transform_impl(name: &str, args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let types: Vec<DataType> = args
        .arg_fields
        .iter()
        .map(|field| field.data_type().clone())
        .collect();
    let target_srid = target_srid_index(&types);
    // A NULL target SRID gives NULL in every row.
    if let Some(index) = target_srid
        && matches!(&args.args[index], ColumnarValue::Scalar(srid) if srid.is_null())
    {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let geometries = geometry_array(&args, 0)?;
    // The source is from_proj when the call has one, otherwise the column's CRS.
    let has_from = args.args.len() == 3;
    let from = if has_from {
        Source::Proj(optional_text_arg(&args, 1, "")?)
    } else {
        let metadata = input_metadata(&args.arg_fields[0]);
        Source::Column {
            definition: proj_definition(metadata.crs()),
            srid: crs_to_srid(metadata.crs()),
        }
    };
    let target = match target_srid {
        Some(_) => {
            let crs = input_metadata(&args.return_field);
            let to = proj_definition(crs.crs())
                .ok_or_else(|| exec_datafusion_err!("{name}: the target SRID has no CRS"))?;
            Target::Srid {
                to,
                to_srid: crs_to_srid(crs.crs()).unwrap_or(SRID_UNKNOWN),
            }
        }
        None => Target::Proj(optional_text_arg(&args, args.args.len() - 1, "")?),
    };
    let kernel = TransformKernel {
        name: name.to_string(),
        from,
        target,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

enum Source {
    Column {
        definition: Option<String>,
        srid: Option<i32>,
    },
    Proj(StringArray),
}

struct TransformKernel {
    name: String,
    from: Source,
    target: Target,
}

impl GeometryKernel for TransformKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let name = self.name.as_str();
        let from = match &self.from {
            Source::Proj(texts) if texts.is_null(row) => return Ok(None),
            Source::Proj(texts) => texts.value(row).to_string(),
            Source::Column { definition, srid } => {
                // PostGIS returns a geometry already in the target SRID as it is.
                if let Target::Srid { to_srid, .. } = &self.target
                    && *srid == Some(*to_srid)
                {
                    return Ok(Some(to_owned_geometry(geom)));
                }
                definition.clone().ok_or_else(|| {
                    exec_datafusion_err!("{name}: Input geometry has unknown (0) SRID")
                })?
            }
        };
        let to = match &self.target {
            Target::Srid { to, .. } => to.clone(),
            Target::Proj(texts) if texts.is_null(row) => return Ok(None),
            Target::Proj(texts) => texts.value(row).to_string(),
        };
        let definition = Definition::CrsToCrs { from, to };
        Ok(Some(transform_geometry(name, geom, &definition)?))
    }
}

/// The geometry with every coordinate transformed by the operation for `definition`.
pub(crate) fn transform_geometry(
    name: &str,
    geom: &impl GeometryTrait<T = f64>,
    definition: &Definition,
) -> Result<Wkt<f64>> {
    with_operation(name, definition, |operation| {
        // The mapping can't fail, so it keeps the first error for afterwards.
        let error: RefCell<Option<DataFusionError>> = RefCell::new(None);
        let result = map_coords(geom, &|coord| match operation.transform(name, coord) {
            Ok(coord) => coord,
            Err(e) => {
                error.borrow_mut().get_or_insert(e);
                coord
            }
        });
        match error.into_inner() {
            Some(e) => Err(e),
            None => Ok(result),
        }
    })?
}
