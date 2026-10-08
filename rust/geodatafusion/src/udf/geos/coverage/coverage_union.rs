//! ST_CoverageUnion, the aggregate.

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
use geos::Geom;
use wkt::Wkt;
use wkt::types::{Dimension, GeometryCollection};

use crate::error::GeoDataFusionResult;
use crate::udf::geos::util::{from_geos, to_geos};
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::util::collect::{
    CollectAccumulator, CollectGroupsAccumulator, collect_groups_accumulator_supported,
    collect_state_fields,
};
use crate::util::field::{input_metadata, wkb_return_field};
use crate::util::signature::single_geometry;

/// Returns the union of a set of polygons forming a coverage.
#[user_doc(
    doc_section(label = "Coverages"),
    description = "Aggregate that computes the union of a set of polygons forming a coverage by removing their shared edges. It is much faster than ST_Union_Agg, but only correct for a valid coverage (polygons that don't overlap and share edges exactly); otherwise the result is invalid, without an error. Empty geometries are left out; NULL is returned if nothing is left. The result is 2D. Unlike PostGIS 3.6, which drops it, the SRID is kept. The geometry[] form isn't supported.",
    syntax_example = "ST_CoverageUnion(geom)",
    argument(name = "geom", description = "geometry")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct CoverageUnion;

impl CoverageUnion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CoverageUnion {
    fn default() -> Self {
        Self::new()
    }
}

impl AggregateUDFImpl for CoverageUnion {
    fn name(&self) -> &str {
        "st_coverageunion"
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
            Arc::new(coverage_union),
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
            Arc::new(coverage_union),
        )?))
    }

    fn order_sensitivity(&self) -> AggregateOrderSensitivity {
        AggregateOrderSensitivity::Insensitive
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn coverage_union(geoms: Vec<Wkt<f64>>) -> GeoDataFusionResult<Option<Wkt<f64>>> {
    let members: Vec<Wkt<f64>> = geoms
        .into_iter()
        .filter(|geom| !is_geometry_topologically_empty(geom))
        .collect();
    if members.is_empty() {
        return Ok(None);
    }
    let collection = Wkt::GeometryCollection(GeometryCollection::new(members, Dimension::XY));
    let union = to_geos(&collection)?.coverage_union()?;
    // PostGIS returns the coverage union in 2D.
    Ok(Some(from_geos(&union, false)?))
}

#[cfg(test)]
mod test {
    use datafusion::prelude::SessionContext;

    use super::*;
    use crate::udf::native::io::GeomFromText;
    use crate::util::test::assert_wkb_output;

    #[tokio::test]
    async fn test_coverage_union_keeps_crs() {
        let ctx = SessionContext::new();
        ctx.register_udaf(CoverageUnion.into());
        ctx.register_udf(GeomFromText::default().into());
        let sql =
            "SELECT ST_CoverageUnion(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))', 4326))";
        assert_wkb_output(&ctx, sql, 4326).await;
    }
}
