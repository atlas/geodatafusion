use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::utils::{AggregateOrderSensitivity, format_state_name};
use datafusion::logical_expr::{Accumulator, AggregateUDFImpl, Documentation, Signature};
use datafusion::scalar::ScalarValue;
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::from_arrow_array;
use geoarrow_array::builder::RectBuilder;
use geoarrow_schema::{BoxType, Dimension, GeoArrowType};

use crate::error::GeoDataFusionResult;
use crate::udf::native::bounding_box::util::bounds::{BoundingRect, total_bounds};
use crate::util::field::input_metadata;
use crate::util::signature::single_geometry;

/// Aggregate function that returns the bounding box of geometries.
#[user_doc(
    doc_section(label = "Bounding Box Functions"),
    description = "An aggregate function that returns the 2D bounding box of a set of geometries, with the CRS of the input. NULL and empty geometries are skipped; the result is NULL if no geometry remains.",
    syntax_example = "ST_Extent(geomfield)",
    argument(name = "geomfield", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Extent;

impl Extent {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Extent {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for Extent {
    fn name(&self) -> &str {
        "st_extent"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field should be called instead")
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        let output_type = BoxType::new(Dimension::XY, input_metadata(&arg_fields[0]));
        Ok(Arc::new(output_type.to_field(self.name(), true)))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        Ok(Box::new(ExtentAccumulator {
            bounds: BoundingRect::new(false),
            input_field: Arc::clone(&args.expr_fields[0]),
            return_field: Arc::clone(&args.return_field),
        }))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        Ok(STATE_NAMES
            .iter()
            .map(|name| {
                Arc::new(Field::new(
                    format_state_name(args.name, name),
                    DataType::Float64,
                    true,
                ))
            })
            .collect())
    }

    fn order_sensitivity(&self) -> AggregateOrderSensitivity {
        // The bounds don't depend on the order of the rows.
        AggregateOrderSensitivity::Insensitive
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The partial state: the bounds so far, ±infinity while nothing was added.
const STATE_NAMES: [&str; 4] = ["xmin", "ymin", "xmax", "ymax"];

#[derive(Debug)]
struct ExtentAccumulator {
    bounds: BoundingRect,
    input_field: FieldRef,
    return_field: FieldRef,
}

impl Accumulator for ExtentAccumulator {
    fn state(&mut self) -> Result<Vec<ScalarValue>> {
        Ok(vec![
            ScalarValue::from(self.bounds.minx()),
            ScalarValue::from(self.bounds.miny()),
            ScalarValue::from(self.bounds.maxx()),
            ScalarValue::from(self.bounds.maxy()),
        ])
    }

    fn evaluate(&mut self) -> Result<ScalarValue> {
        Ok(extent_evaluate(&self.bounds, &self.return_field)?)
    }

    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        Ok(extent_update(
            &mut self.bounds,
            &values[0],
            &self.input_field,
        )?)
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        let min = |array: &ArrayRef| {
            arrow_arith::aggregate::min(array.as_primitive::<Float64Type>())
                .unwrap_or(f64::INFINITY)
        };
        let max = |array: &ArrayRef| {
            arrow_arith::aggregate::max(array.as_primitive::<Float64Type>())
                .unwrap_or(f64::NEG_INFINITY)
        };
        self.bounds.update(&BoundingRect::from_xy(
            min(&states[0]),
            min(&states[1]),
            max(&states[2]),
            max(&states[3]),
        ));
        Ok(())
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

fn extent_update(
    bounds: &mut BoundingRect,
    array: &ArrayRef,
    field: &FieldRef,
) -> GeoDataFusionResult<()> {
    let geometries = from_arrow_array(array, field)?;
    bounds.update(&total_bounds(geometries.as_ref())?);
    Ok(())
}

/// The box of the bounds, or NULL if nothing was added: no rows, or only NULL or EMPTY ones.
fn extent_evaluate(
    bounds: &BoundingRect,
    return_field: &Field,
) -> GeoDataFusionResult<ScalarValue> {
    if bounds.is_empty() {
        return Ok(ScalarValue::try_from(return_field.data_type())?);
    }
    let GeoArrowType::Rect(output_type) = GeoArrowType::from_arrow_field(return_field)? else {
        return Err(internal_datafusion_err!("st_extent: unexpected return field").into());
    };
    let mut builder = RectBuilder::with_capacity(output_type, 1);
    builder.push_rect(Some(bounds));
    Ok(ScalarValue::try_from_array(
        &builder.finish().into_array_ref(),
        0,
    )?)
}

#[cfg(test)]
mod test {
    use arrow_array::Array;
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int64Type;
    use datafusion::prelude::{SessionConfig, SessionContext};

    use super::*;
    use crate::udf::native::io::GeomFromText;

    /// Partial aggregation materialises the state, which needs `state_fields`.
    #[tokio::test]
    async fn test_group_by_with_partitions() {
        let ctx = SessionContext::new_with_config(SessionConfig::new().with_target_partitions(4));
        ctx.register_udaf(Extent.into());
        ctx.register_udf(GeomFromText::new().into());

        let sql = "SELECT k, ST_Extent(ST_GeomFromText(g)) AS extent \
                   FROM (VALUES (1, 'POINT(1 2)'), (2, 'POINT(5 5)'), (1, 'POINT(3 -4)'), (3, NULL)) \
                   AS t(k, g) GROUP BY k ORDER BY k";
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        let batch =
            datafusion::arrow::compute::concat_batches(&batches[0].schema(), &batches).unwrap();
        let keys = batch.column(0).as_primitive::<Int64Type>();
        let extents = batch.column(1).as_struct();
        let xmin = extents.column(0).as_primitive::<Float64Type>();
        let ymax = extents.column(3).as_primitive::<Float64Type>();

        assert_eq!(keys.values(), &[1, 2, 3]);
        assert_eq!((xmin.value(0), ymax.value(0)), (1.0, 2.0));
        assert_eq!((xmin.value(1), ymax.value(1)), (5.0, 5.0));
        assert!(extents.is_null(2));
    }
}
