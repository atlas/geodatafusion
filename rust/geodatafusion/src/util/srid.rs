//! Conversions between PostGIS SRIDs and GeoArrow CRSs.
//!
//! PostGIS stores an SRID per value; GeoArrow stores a CRS per column. geodatafusion keeps the
//! CRS as an `AUTHORITY:CODE` string in memory (`EPSG:4326`), which round-trips through
//! GeoPandas, DuckDB and GDAL in Arrow hand-offs, and reads every form other producers write.

use geoarrow_schema::crs::{Crs, CrsType};
use serde_json::Value;

use crate::util::srid_authorities::SRID_AUTHORITIES;

/// PostGIS's "unknown" SRID, stored as no CRS.
pub(crate) const SRID_UNKNOWN: i32 = 0;

/// The largest SRID PostGIS stores; larger values are folded into the reserved range.
const SRID_MAXIMUM: i64 = 999_999;

/// The largest SRID PostGIS leaves for users.
const SRID_USER_MAXIMUM: i64 = 998_999;

/// The authority that defines an SRID in PostGIS's `spatial_ref_sys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Authority {
    Epsg,
    Esri,
}

impl Authority {
    fn name(self) -> &'static str {
        match self {
            Authority::Epsg => "EPSG",
            Authority::Esri => "ESRI",
        }
    }
}

/// Clamps an SRID the way PostGIS does: values `<= 0` become [`SRID_UNKNOWN`], and values above
/// 999999 are folded into the reserved range above 999000 (PostGIS emits a NOTICE for both).
pub(crate) fn clamp_srid(srid: i64) -> i32 {
    let clamped = if srid <= 0 {
        0
    } else if srid > SRID_MAXIMUM {
        SRID_USER_MAXIMUM + 1 + srid % (SRID_MAXIMUM - SRID_USER_MAXIMUM - 1)
    } else {
        srid
    };
    i32::try_from(clamped).expect("a clamped SRID is at most 999999")
}

/// The authority that defines `srid` in PostGIS, if any.
fn authority(srid: i32) -> Option<Authority> {
    let index = SRID_AUTHORITIES.partition_point(|(_, last, _)| *last < srid);
    SRID_AUTHORITIES
        .get(index)
        .filter(|(first, _, _)| *first <= srid)
        .map(|(_, _, authority)| *authority)
}

/// The name of the authority that defines `srid` in PostGIS (`EPSG`, `ESRI`), if any. Output
/// formats that name the CRS (GeoJSON, GML) need it.
pub(crate) fn srid_authority_name(srid: i32) -> Option<&'static str> {
    authority(srid).map(Authority::name)
}

/// The GeoArrow CRS for a PostGIS SRID: no CRS for [`SRID_UNKNOWN`], `EPSG:n` or `ESRI:n` for
/// SRIDs PostGIS defines, and an opaque `srid` CRS for anything else.
pub(crate) fn srid_to_crs(srid: i32) -> Crs {
    if srid == SRID_UNKNOWN {
        return Crs::default();
    }
    match authority(srid) {
        Some(authority) => Crs::from_authority_code(format!("{}:{srid}", authority.name())),
        None => Crs::from_srid(srid.to_string()),
    }
}

/// The PostGIS SRID of a GeoArrow CRS: [`SRID_UNKNOWN`] for no CRS, the code of an `EPSG` or
/// `ESRI` authority code, an `srid` CRS, or the `id` of a PROJJSON CRS, and 4326 for
/// `OGC:CRS84`. `None` for a CRS that doesn't name an SRID.
pub(crate) fn crs_to_srid(crs: &Crs) -> Option<i32> {
    let Some(value) = crs.crs_value() else {
        return Some(SRID_UNKNOWN);
    };
    match (crs.crs_type(), value) {
        (Some(CrsType::Srid), Value::String(srid)) => srid.parse().ok(),
        (Some(CrsType::Projjson), Value::Object(_)) => projjson_srid(value),
        // Some readers label a serialized PROJJSON string as PROJJSON.
        (Some(CrsType::Projjson) | None, Value::String(text)) if text.starts_with('{') => {
            projjson_srid(&serde_json::from_str(text).ok()?)
        }
        (Some(CrsType::AuthorityCode) | None, Value::String(code)) => {
            let (authority, code) = code.split_once(':')?;
            authority_srid(authority, &Value::String(code.to_string()))
        }
        _ => None,
    }
}

/// The SRID named by a PROJJSON object's `id` (or the first of its `ids`).
fn projjson_srid(projjson: &Value) -> Option<i32> {
    let id = projjson
        .get("id")
        .or_else(|| projjson.get("ids").and_then(|ids| ids.get(0)))?;
    authority_srid(id.get("authority")?.as_str()?, id.get("code")?)
}

fn authority_srid(authority: &str, code: &Value) -> Option<i32> {
    let code = match code {
        Value::Number(number) => number.to_string(),
        Value::String(code) => code.clone(),
        _ => return None,
    };
    if authority.eq_ignore_ascii_case("OGC") && code.eq_ignore_ascii_case("CRS84") {
        // PostGIS stores WGS 84 longitude/latitude as 4326.
        return Some(4326);
    }
    if authority.eq_ignore_ascii_case("EPSG") || authority.eq_ignore_ascii_case("ESRI") {
        return code.parse().ok();
    }
    None
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_clamp_srid_matches_postgis() {
        // Expected values from PostGIS 3.6.4: ST_SRID(ST_SetSRID('POINT(1 1)', s)).
        for (srid, expected) in [
            (-5, 0),
            (-1, 0),
            (0, 0),
            (1, 1),
            (4326, 4326),
            (998999, 998999),
            (999000, 999000),
            (999999, 999999),
            (1000000, 999001),
            (1000001, 999002),
            (2000000, 999002),
            (99999999, 999099),
        ] {
            assert_eq!(clamp_srid(srid), expected, "clamp_srid({srid})");
        }
    }

    #[test]
    fn test_srid_to_crs_uses_postgis_authorities() {
        assert_eq!(srid_to_crs(0), Crs::default());
        assert_eq!(
            srid_to_crs(4326),
            Crs::from_authority_code("EPSG:4326".to_string())
        );
        assert_eq!(
            srid_to_crs(102003),
            Crs::from_authority_code("ESRI:102003".to_string())
        );
        // Not in spatial_ref_sys.
        assert_eq!(srid_to_crs(123456), Crs::from_srid("123456".to_string()));
    }

    #[test]
    fn test_crs_to_srid_reads_every_form() {
        assert_eq!(crs_to_srid(&Crs::default()), Some(0));
        assert_eq!(
            crs_to_srid(&Crs::from_authority_code("EPSG:2263".to_string())),
            Some(2263)
        );
        assert_eq!(
            crs_to_srid(&Crs::from_authority_code("OGC:CRS84".to_string())),
            Some(4326)
        );
        assert_eq!(crs_to_srid(&Crs::from_srid("3857".to_string())), Some(3857));
        let projjson = json!({"type": "GeographicCRS", "id": {"authority": "EPSG", "code": 4326}});
        assert_eq!(
            crs_to_srid(&Crs::from_projjson(projjson.clone())),
            Some(4326)
        );
        assert_eq!(
            crs_to_srid(&Crs::from_unknown_crs_type(projjson.to_string())),
            Some(4326)
        );
        let crs84 = json!({"id": {"authority": "OGC", "code": "CRS84"}});
        assert_eq!(crs_to_srid(&Crs::from_projjson(crs84)), Some(4326));
        assert_eq!(
            crs_to_srid(&Crs::from_wkt2_2019("GEOGCRS[...]".to_string())),
            None
        );
    }

    #[test]
    fn test_srid_round_trips() {
        for srid in [0, 2263, 3857, 4326, 102003, 123456] {
            assert_eq!(crs_to_srid(&srid_to_crs(srid)), Some(srid));
        }
    }
}
