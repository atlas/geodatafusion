//! Helpers for unit tests.

use datafusion::prelude::SessionContext;
use geoarrow_schema::WkbType;
use geoarrow_schema::crs::Crs;

/// Runs `sql`, which must return one geometry column, and asserts that the column is WKB with
/// the CRS `EPSG:<srid>`.
pub(crate) async fn assert_wkb_output(ctx: &SessionContext, sql: &str, srid: i32) {
    let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
    let field = batches[0].schema().field(0).clone();
    let wkb_type = field
        .try_extension_type::<WkbType>()
        .unwrap_or_else(|e| panic!("{sql}: expected a WKB column, got {field:?}: {e}"));
    assert_eq!(
        wkb_type.metadata().crs(),
        &Crs::from_authority_code(format!("EPSG:{srid}")),
        "{sql}"
    );
}
