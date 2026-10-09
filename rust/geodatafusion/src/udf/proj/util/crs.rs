//! The PROJ definition of a GeoArrow CRS.

use geoarrow_schema::crs::{Crs, CrsType};
use serde_json::Value;

use crate::util::srid::srid_authority_name;

/// What `proj_create` should read for a column's CRS: an authority code as it is (`EPSG:4326`),
/// an SRID with its authority, PROJJSON as JSON text, and WKT as it is. `None` for no CRS (SRID
/// 0), which PostGIS can't transform either.
pub(crate) fn proj_definition(crs: &Crs) -> Option<String> {
    let value = crs.crs_value()?;
    Some(match (crs.crs_type(), value) {
        (Some(CrsType::Srid), Value::String(srid)) => {
            let authority = srid
                .parse()
                .ok()
                .and_then(srid_authority_name)
                .unwrap_or("EPSG");
            format!("{authority}:{srid}")
        }
        (_, Value::String(text)) => text.clone(),
        (_, value) => value.to_string(),
    })
}
