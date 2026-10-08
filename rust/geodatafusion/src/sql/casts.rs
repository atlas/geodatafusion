use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::config::ConfigOptions;
use datafusion::common::tree_node::Transformed;
use datafusion::common::{DFSchema, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::expr::{Cast, ScalarFunction};
use datafusion::logical_expr::expr_rewriter::FunctionRewrite;
use datafusion::logical_expr::{Expr, ExprSchemable, ScalarUDF, ScalarUDFImpl};
use geoarrow_schema::{BoxType, Dimension, Metadata, WkbType};

use crate::udf::native::bounding_box::{Box2D, Box3D};
use crate::udf::native::io::{AsEWKB, AsHEXEWKB};
use crate::udf::native::types::Geometry;
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// Turns casts to and from the spatial types into calls, before type coercion.
///
/// | Cast | Becomes |
/// |---|---|
/// | `x::geometry[(type, srid)]` | `geometry(x)`, checking the SRID against the type's |
/// | `x::box2d`, `x::box3d` | `box2d(x)`, `box3d(x)` |
/// | `geom::text` | `st_ashexewkb(geom)`, PostGIS's `text(geometry)` |
/// | `geom::bytea` | `st_asewkb(geom)` |
///
/// A cast of a geometry to any other type is a plan error. Without this, DataFusion would cast
/// the storage: text bytes as WKB, or WKB as text.
#[derive(Debug)]
pub(crate) struct GeoCastRewrite;

impl FunctionRewrite for GeoCastRewrite {
    fn name(&self) -> &str {
        "geodatafusion_casts"
    }

    fn rewrite(
        &self,
        expr: Expr,
        schema: &DFSchema,
        _config: &ConfigOptions,
    ) -> Result<Transformed<Expr>> {
        let Expr::Cast(Cast {
            expr: input,
            field: target,
        }) = &expr
        else {
            return Ok(Transformed::no(expr));
        };
        if let Some(udf) = cast_to(target) {
            return Ok(Transformed::yes(call(udf, *input.clone())));
        }
        let (_, source) = input.to_field(schema)?;
        if is_spatial(&source) {
            return cast_from(&source, target, *input.clone()).map(Transformed::yes);
        }
        Ok(Transformed::no(expr))
    }
}

/// The function converting a value to a spatial target type.
fn cast_to(target: &Field) -> Option<Arc<ScalarUDF>> {
    if let Ok(wkb_type) = target.try_extension_type::<WkbType>() {
        let srid = typmod_srid(wkb_type.metadata());
        return Some(udf(Geometry::with_srid(srid)));
    }
    let box_type = target.try_extension_type::<BoxType>().ok()?;
    Some(match box_type.dimension() {
        Dimension::XYZ => udf(Box3D::new()),
        _ => udf(Box2D::new()),
    })
}

/// The type planner gives `geometry(type, srid)` the SRID's CRS.
fn typmod_srid(metadata: &Metadata) -> i32 {
    crs_to_srid(metadata.crs()).unwrap_or(SRID_UNKNOWN)
}

fn cast_from(source: &Field, target: &Field, input: Expr) -> Result<Expr> {
    let (output, output_type) = match target.data_type() {
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            (call(udf(AsHEXEWKB::new()), input), DataType::Utf8)
        }
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => {
            (call(udf(AsEWKB::new()), input), DataType::Binary)
        }
        other => {
            let name = source.extension_type_name().unwrap_or_default();
            return plan_err!("Cannot cast {name} to {other}");
        }
    };
    if target.data_type() == &output_type {
        return Ok(output);
    }
    // A view or large target type.
    Ok(Expr::Cast(Cast::new(
        Box::new(output),
        target.data_type().clone(),
    )))
}

fn is_spatial(field: &Field) -> bool {
    field
        .extension_type_name()
        .is_some_and(|name| name.starts_with("geoarrow."))
}

fn udf(function: impl ScalarUDFImpl + 'static) -> Arc<ScalarUDF> {
    Arc::new(ScalarUDF::new_from_impl(function))
}

fn call(udf: Arc<ScalarUDF>, input: Expr) -> Expr {
    Expr::ScalarFunction(ScalarFunction::new_udf(udf, vec![input]))
}
