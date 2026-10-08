//! Aggregates that collect a group's geometries and compute their result once, as PostGIS's do:
//! an array-building transition function, then the `geometry[]` overload as the final function.
//!
//! The state, `ORDER BY` inside the call, `DISTINCT` and merging are delegated to DataFusion's
//! `array_agg`. An aggregate only supplies the final step, a [`Finalize`] function from a group's
//! geometries to its result.

use std::fmt::Debug;
use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef, BooleanArray, ListArray};
use arrow_schema::{Field, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::functions_aggregate::array_agg::ArrayAgg;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::{Accumulator, AggregateUDFImpl, EmitTo, GroupsAccumulator};
use datafusion::scalar::ScalarValue;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::from_arrow_array;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::GeoArrowType;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::kernel::map_geometry;
use crate::util::owned::ToOwned;

/// The result for one group from its geometries, in input order (or the call's `ORDER BY`), NULLs
/// left out. `None` is SQL NULL. Never called for a group without geometries, whose result is
/// NULL, as in PostGIS.
///
/// Constant further arguments, such as ST_Union_Agg's grid size, are captured when the
/// accumulator is created.
pub(crate) type Finalize =
    Arc<dyn Fn(Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> + Send + Sync>;

/// The `state_fields` of a collect aggregate.
pub(crate) fn collect_state_fields(args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
    ArrayAgg::default().state_fields(args)
}

/// The `groups_accumulator_supported` of a collect aggregate.
pub(crate) fn collect_groups_accumulator_supported(args: AccumulatorArgs) -> bool {
    ArrayAgg::default().groups_accumulator_supported(args)
}

/// The accumulator for one group at a time.
#[derive(Debug)]
pub(crate) struct CollectAccumulator {
    inner: Box<dyn Accumulator>,
    arguments: usize,
    finisher: Finisher,
}

impl CollectAccumulator {
    pub(crate) fn try_new(args: AccumulatorArgs, finalize: Finalize) -> Result<Self> {
        let finisher = Finisher::new(&args, finalize);
        let arguments = args.exprs.len();
        let inner = ArrayAgg::default().accumulator(skip_nulls(args))?;
        Ok(Self {
            inner,
            arguments,
            finisher,
        })
    }
}

impl Accumulator for CollectAccumulator {
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        self.inner
            .update_batch(&without_constants(values, self.arguments))
    }

    fn evaluate(&mut self) -> Result<ScalarValue> {
        let ScalarValue::List(lists) = self.inner.evaluate()? else {
            return internal_err!("array_agg evaluates to a list");
        };
        let result = self.finisher.finish(&lists)?;
        ScalarValue::try_from_array(&result, 0)
    }

    fn size(&self) -> usize {
        self.inner.size() + size_of::<Finisher>()
    }

    fn state(&mut self) -> Result<Vec<ScalarValue>> {
        self.inner.state()
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        self.inner.merge_batch(states)
    }
}

/// The accumulator for many groups at once.
pub(crate) struct CollectGroupsAccumulator {
    inner: Box<dyn GroupsAccumulator>,
    arguments: usize,
    finisher: Finisher,
}

impl CollectGroupsAccumulator {
    pub(crate) fn try_new(args: AccumulatorArgs, finalize: Finalize) -> Result<Self> {
        let finisher = Finisher::new(&args, finalize);
        let arguments = args.exprs.len();
        let inner = ArrayAgg::default().create_groups_accumulator(skip_nulls(args))?;
        Ok(Self {
            inner,
            arguments,
            finisher,
        })
    }
}

impl GroupsAccumulator for CollectGroupsAccumulator {
    fn update_batch(
        &mut self,
        values: &[ArrayRef],
        group_indices: &[usize],
        opt_filter: Option<&BooleanArray>,
        total_num_groups: usize,
    ) -> Result<()> {
        self.inner.update_batch(
            &without_constants(values, self.arguments),
            group_indices,
            opt_filter,
            total_num_groups,
        )
    }

    fn evaluate(&mut self, emit_to: EmitTo) -> Result<ArrayRef> {
        let lists = self.inner.evaluate(emit_to)?;
        self.finisher.finish(lists.as_list())
    }

    fn state(&mut self, emit_to: EmitTo) -> Result<Vec<ArrayRef>> {
        self.inner.state(emit_to)
    }

    fn merge_batch(
        &mut self,
        values: &[ArrayRef],
        group_indices: &[usize],
        total_num_groups: usize,
    ) -> Result<()> {
        self.inner
            .merge_batch(values, group_indices, total_num_groups)
    }

    fn convert_to_state(
        &self,
        values: &[ArrayRef],
        opt_filter: Option<&BooleanArray>,
    ) -> Result<Vec<ArrayRef>> {
        self.inner
            .convert_to_state(&without_constants(values, self.arguments), opt_filter)
    }

    fn size(&self) -> usize {
        self.inner.size() + size_of::<Finisher>()
    }
}

/// The input without the constant arguments after the geometry, which aren't stored. The values
/// of the call's `ORDER BY` expressions follow the `arguments` arguments, and stay.
fn without_constants(values: &[ArrayRef], arguments: usize) -> Vec<ArrayRef> {
    std::iter::once(&values[0])
        .chain(&values[arguments..])
        .cloned()
        .collect()
}

/// PostGIS aggregates skip NULL input, so NULLs aren't stored.
fn skip_nulls(args: AccumulatorArgs) -> AccumulatorArgs {
    AccumulatorArgs {
        ignore_nulls: true,
        ..args
    }
}

/// Applies a [`Finalize`] to each group's list.
struct Finisher {
    /// The input's field: `array_agg` stores the input's Arrow type, but not its extension
    /// metadata, which is needed to read it.
    item_field: FieldRef,
    return_field: FieldRef,
    finalize: Finalize,
}

impl Debug for Finisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Finisher")
            .field("item_field", &self.item_field)
            .field("return_field", &self.return_field)
            .finish_non_exhaustive()
    }
}

impl Finisher {
    fn new(args: &AccumulatorArgs, finalize: Finalize) -> Self {
        Self {
            item_field: Arc::clone(&args.expr_fields[0]),
            return_field: Arc::clone(&args.return_field),
            finalize,
        }
    }

    /// One result per list, as WKB of the return field's type.
    fn finish(&self, lists: &ListArray) -> Result<ArrayRef> {
        Ok(self.finish_lists(lists)?)
    }

    fn finish_lists(&self, lists: &ListArray) -> GeoDataFusionResult<ArrayRef> {
        let GeoArrowType::Wkb(wkb_type) = GeoArrowType::from_arrow_field(&self.return_field)?
        else {
            return Err(datafusion::common::internal_datafusion_err!(
                "a collect aggregate returns WKB, got {:?}",
                self.return_field
            )
            .into());
        };
        let mut builder = WkbBuilder::<i32>::new(wkb_type);
        for row in 0..lists.len() {
            let result = if lists.is_null(row) {
                None
            } else {
                self.finish_list(&lists.value(row))?
            };
            builder.push_geometry(result.as_ref())?;
        }
        Ok(builder.finish().to_array_ref())
    }

    fn finish_list(&self, values: &ArrayRef) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let field = Field::new("", values.data_type().clone(), true)
            .with_metadata(self.item_field.metadata().clone());
        let geometries = from_arrow_array(values, &field)?;
        let geometries: Vec<Option<Wkt<f64>>> = map_geometry(geometries.as_ref(), &ToOwned)?;
        let geometries: Vec<Wkt<f64>> = geometries.into_iter().flatten().collect();
        if geometries.is_empty() {
            return Ok(None);
        }
        (self.finalize)(geometries)
    }
}
