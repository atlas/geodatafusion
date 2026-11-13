use std::sync::Arc;

use arrow_schema::{DataType, Field};
use datafusion::common::config::ConfigOptions;
use datafusion::common::tree_node::Transformed;
use datafusion::common::{DFSchema, ScalarValue, plan_err};
use datafusion::error::Result;
use datafusion::logical_expr::expr::{Cast, ScalarFunction};
use datafusion::logical_expr::expr_rewriter::FunctionRewrite;
use datafusion::logical_expr::{Expr, ExprSchemable, ScalarUDF, ScalarUDFImpl};
use geoarrow_schema::{BoxType, Dimension, Edges, Metadata, WkbType};

use crate::udf::native::bounding_box::{Box2D, Box2DOut, Box3D, Box3DOut};
use crate::udf::native::io::{AsEWKB, AsHEXEWKB};
use crate::udf::native::types::{Geography, Geometry};
use crate::util::srid::{SRID_UNKNOWN, crs_to_srid};

/// Turns casts to and from the spatial types into calls, before type coercion.
///
/// | Cast | Becomes |
/// |---|---|
/// | `x::geometry[(type, srid)]` | `geometry(x)`, checking the SRID against the type's |
/// | `x::geography[(type, srid)]` | `geography(x)`, likewise |
/// | `x::box2d`, `x::box3d` | `box2d(x)`, `box3d(x)` |
/// | `geom::text` | `st_ashexewkb(geom)`, PostGIS's `text(geometry)` |
/// | `geom::bytea` | `st_asewkb(geom)` |
/// | `box::text` | `box2d_out(box)` or `box3d_out(box)`, by the box's dimension |
///
/// A cast of a geometry to its own GeoArrow type and storage is left as it is: it changes only
/// what the field says of the values, such as the CRS of a native geometry read from a source
/// that keeps none, and DataFusion casts it without touching them. A cast of a geometry to any
/// other type, or of a box to anything but text, is a plan error. Without this, DataFusion would
/// cast the storage: text bytes as WKB, or WKB as text.
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
        if is_spatial(&source) && !restates(&source, target) {
            return cast_from(&source, target, *input.clone()).map(Transformed::yes);
        }
        Ok(Transformed::no(expr))
    }
}

/// Whether a cast of `source` to `target` keeps its GeoArrow type and storage, changing only the
/// metadata, such as the CRS.
fn restates(source: &Field, target: &Field) -> bool {
    is_spatial(target)
        && source.extension_type_name() == target.extension_type_name()
        && source.data_type() == target.data_type()
}

/// The function converting a value to a spatial target type.
pub(super) fn cast_to(target: &Field) -> Option<Arc<ScalarUDF>> {
    if let Ok(wkb_type) = target.try_extension_type::<WkbType>() {
        let srid = typmod_srid(wkb_type.metadata());
        if wkb_type.metadata().edges() == Some(Edges::Spherical) {
            return Some(udf(Geography::with_srid(srid)));
        }
        return Some(udf(Geometry::with_srid(srid)));
    }
    let box_type = target.try_extension_type::<BoxType>().ok()?;
    Some(match box_type.dimension() {
        Dimension::XYZ => udf(Box3D::new()),
        _ => udf(Box2D::new()),
    })
}

/// The type planner gives `geometry(type, srid)` and `geography(type, srid)` the SRID's CRS.
fn typmod_srid(metadata: &Metadata) -> i32 {
    crs_to_srid(metadata.crs()).unwrap_or(SRID_UNKNOWN)
}

fn cast_from(source: &Field, target: &Field, input: Expr) -> Result<Expr> {
    let (output, output_type) = match target.data_type() {
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            let output = match source.try_extension_type::<BoxType>() {
                Ok(box_type) if box_type.dimension() == Dimension::XYZ => udf(Box3DOut::new()),
                Ok(_) => udf(Box2DOut::new()),
                Err(_) => udf(AsHEXEWKB::new()),
            };
            (call(output, input), DataType::Utf8)
        }
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView
            if source.try_extension_type::<BoxType>().is_err() =>
        {
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

/// What a folded cast of text to a spatial type should have been: the call [`GeoCastRewrite`]
/// would have made of the cast.
///
/// DataFusion simplifies table function arguments before analysis, so `'POINT(1 1)'::geometry`
/// reaches a table function as the text's bytes typed as WKB. WKB starts with its byte order, 0
/// or 1, and WKT, EWKT and HEXEWKB with a printable character, so the two can't be confused.
pub(crate) fn unfold_text_cast(expr: Expr) -> Expr {
    let Expr::Literal(ScalarValue::Binary(Some(bytes)), Some(metadata)) = &expr else {
        return expr;
    };
    if matches!(bytes.first(), None | Some(0 | 1)) {
        return expr;
    }
    let field = metadata.add_to_field(Field::new("", DataType::Binary, true));
    let (Some(udf), Ok(text)) = (cast_to(&field), std::str::from_utf8(bytes)) else {
        return expr;
    };
    call(
        udf,
        Expr::Literal(ScalarValue::Utf8(Some(text.to_string())), None),
    )
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use arrow_array::RecordBatch;
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int32Type;
    use arrow_schema::Schema;
    use datafusion::prelude::{SessionContext, col};
    use geoarrow_array::GeoArrowArray;
    use geoarrow_array::test::point;
    use geoarrow_schema::{CoordType, GeoArrowType, LineStringType};

    use super::*;
    use crate::util::srid::srid_to_crs;

    /// A native point column `geom` with `metadata`.
    fn point_field(metadata: Metadata) -> Field {
        point::array(CoordType::Separated, Dimension::XY)
            .with_metadata(Arc::new(metadata))
            .data_type()
            .to_field("geom", true)
    }

    /// A table `t` of native points without a CRS.
    fn points() -> SessionContext {
        let ctx = SessionContext::new();
        crate::register(&ctx);
        let points = point::array(CoordType::Separated, Dimension::XY);
        let schema = Schema::new([Arc::new(point_field(Metadata::default()))]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![points.to_array_ref()]).unwrap();
        ctx.register_batch("t", batch).unwrap();
        ctx
    }

    fn cast(target: Field) -> Expr {
        Expr::Cast(Cast::new_from_field(
            Box::new(col("geom")),
            Arc::new(target),
        ))
        .alias("geom")
    }

    #[tokio::test]
    async fn test_cast_to_own_type_keeps_values_and_states_crs() {
        let ctx = points();
        let in_4326 = point_field(Metadata::new(srid_to_crs(4326), None));
        let restated = ctx
            .table("t")
            .await
            .unwrap()
            .select(vec![cast(in_4326.clone())])
            .unwrap();
        let batches = restated.clone().collect().await.unwrap();
        assert_eq!(batches[0].schema().field(0).metadata(), in_4326.metadata());
        let original = ctx.table("t").await.unwrap().collect().await.unwrap();
        assert_eq!(batches[0].column(0), original[0].column(0));

        ctx.register_table("restated", restated.into_view())
            .unwrap();
        let srids = ctx
            .sql("SELECT ST_SRID(geom) FROM restated")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let srids = srids[0].column(0).as_primitive::<Int32Type>();
        assert!(srids.iter().flatten().count() > 0);
        assert!(srids.iter().flatten().all(|srid| srid == 4326));
    }

    #[tokio::test]
    async fn test_cast_to_another_geometry_type_is_an_error() {
        let ctx = points();
        let lines =
            GeoArrowType::LineString(LineStringType::new(Dimension::XY, Default::default()))
                .to_field("geom", true);
        let error = ctx
            .table("t")
            .await
            .unwrap()
            .select(vec![cast(lines)])
            .unwrap()
            .collect()
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Cannot cast geoarrow.point"),
            "{error}"
        );
    }
}
