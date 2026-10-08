//! ST_Polygonize, the aggregate.

use std::sync::Arc;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::utils::AggregateOrderSensitivity;
use datafusion::logical_expr::{
    Accumulator, AggregateUDFImpl, Documentation, GroupsAccumulator, Signature,
};
use datafusion_macros::user_doc;
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, has_z, to_geos};
use crate::util::collect::{
    CollectAccumulator, CollectGroupsAccumulator, collect_groups_accumulator_supported,
    collect_state_fields,
};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::single_geometry;

/// Returns the polygons formed by the linework of a set of geometries.
#[user_doc(
    doc_section(label = "Geometry Processing"),
    description = "Aggregate that computes a geometry collection of the polygons formed by the linework of a set of geometries. Lines must be noded (meet at their endpoints) to form polygons. Without any polygon, the result is an empty geometry collection. NULLs are skipped, and no geometries give NULL. This function keeps Z and drops M. The geometry[] form isn't supported.",
    syntax_example = "ST_Polygonize(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Polygonize;

impl Polygonize {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Polygonize {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for Polygonize {
    fn name(&self) -> &str {
        "st_polygonize"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field should be called instead")
    }

    fn return_field(&self, arg_fields: &[FieldRef]) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&arg_fields[0]),
        ))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        Ok(Box::new(CollectAccumulator::try_new(
            args,
            Arc::new(polygonize),
        )?))
    }

    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        collect_state_fields(args)
    }

    fn groups_accumulator_supported(&self, args: AccumulatorArgs) -> bool {
        collect_groups_accumulator_supported(args)
    }

    fn create_groups_accumulator(
        &self,
        args: AccumulatorArgs,
    ) -> Result<Box<dyn GroupsAccumulator>> {
        Ok(Box::new(CollectGroupsAccumulator::try_new(
            args,
            Arc::new(polygonize),
        )?))
    }

    fn order_sensitivity(&self) -> AggregateOrderSensitivity {
        AggregateOrderSensitivity::Insensitive
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn polygonize(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    // PostGIS keeps a Z from GEOS only when an input has Z.
    let want_z = geoms.iter().any(has_z);
    let geoms = geoms
        .iter()
        .map(to_geos)
        .collect::<GeoDataFusionResult<Vec<_>>>()?;
    let polygons = geos::Geometry::polygonize(&geoms)?;
    Ok(Some(from_geos(&polygons, want_z)?))
}
