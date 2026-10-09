//! ST_TransformPipeline and ST_InverseTransformPipeline: coordinates transformed by a PROJ
//! pipeline or coordinate operation.

use std::sync::{Arc, LazyLock};

use arrow_array::{Array, StringArray, new_null_array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use geoarrow_schema::Metadata;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::proj::srs::transform::transform_geometry;
use crate::udf::proj::util::operation::Definition;
use crate::util::args::{optional_text_arg, scalar_srid};
use crate::util::field::{geometry_array, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_TransformPipeline(geometry geom, text pipeline, integer to_srid = 0), and the
/// same for ST_InverseTransformPipeline.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Text],
    &[Arg::Geometry, Arg::Text, Arg::Srid],
];

static SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["geom", "pipeline", "to_srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Return a new geometry with coordinates transformed to a different spatial reference system
/// using a defined coordinate transformation pipeline.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Returns the geometry with its coordinates transformed by a PROJ pipeline or coordinate operation, such as urn:ogc:def:coordinateOperation:EPSG::16031 or a +proj=pipeline string, run forward. The result has the SRID to_srid (default 0), which must be a constant. Z is transformed too and M kept. Requires the proj feature.",
    syntax_example = "ST_TransformPipeline(geom, pipeline, to_srid)",
    argument(name = "geom", description = "geometry"),
    argument(name = "pipeline", description = "text"),
    argument(name = "to_srid", description = "integer, default 0"),
    related_udf(name = "st_transform"),
    related_udf(name = "st_inversetransformpipeline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct TransformPipeline;

impl TransformPipeline {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for TransformPipeline {
    fn name(&self) -> &str {
        "st_transformpipeline"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        pipeline_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(pipeline_impl(self.name(), args, false)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Return a new geometry with coordinates transformed to a different spatial reference system
/// using the inverse of a defined coordinate transformation pipeline.
#[user_doc(
    doc_section(label = "Spatial Reference System Functions"),
    description = "Returns the geometry with its coordinates transformed by the inverse of a PROJ pipeline or coordinate operation. The result has the SRID to_srid (default 0), which must be a constant. Z is transformed too and M kept. Requires the proj feature.",
    syntax_example = "ST_InverseTransformPipeline(geom, pipeline, to_srid)",
    argument(name = "geom", description = "geometry"),
    argument(name = "pipeline", description = "text"),
    argument(name = "to_srid", description = "integer, default 0"),
    related_udf(name = "st_transformpipeline")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct InverseTransformPipeline;

impl InverseTransformPipeline {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InverseTransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for InverseTransformPipeline {
    fn name(&self) -> &str {
        "st_inversetransformpipeline"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        pipeline_return_field(self.name(), &args)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(pipeline_impl(self.name(), args, true)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// WKB with the CRS of `to_srid`, or none.
fn pipeline_return_field(name: &str, args: &ReturnFieldArgs) -> Result<FieldRef> {
    let crs = if args.arg_fields.len() > 2 {
        scalar_srid(name, args, 2)?
            .map(srid_to_crs)
            .unwrap_or_default()
    } else {
        Default::default()
    };
    Ok(wkb_return_field(name, Arc::new(Metadata::new(crs, None))))
}

fn pipeline_impl(
    name: &str,
    args: ScalarFunctionArgs,
    inverse: bool,
) -> GeoDataFusionResult<ColumnarValue> {
    // A NULL to_srid gives NULL in every row.
    if matches!(args.args.get(2), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let geometries = geometry_array(&args, 0)?;
    let kernel = PipelineKernel {
        name: name.to_string(),
        pipelines: optional_text_arg(&args, 1, "")?,
        inverse,
    };
    let result = map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?;
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct PipelineKernel {
    name: String,
    pipelines: StringArray,
    inverse: bool,
}

impl GeometryKernel for PipelineKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        if self.pipelines.is_null(row) {
            return Ok(None);
        }
        let definition = Definition::Pipeline {
            definition: self.pipelines.value(row).to_string(),
            inverse: self.inverse,
        };
        Ok(Some(transform_geometry(&self.name, geom, &definition)?))
    }
}
