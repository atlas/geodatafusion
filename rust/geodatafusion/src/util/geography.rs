//! PostGIS's geography type: a geometry field whose GeoArrow metadata has spherical edges, with
//! a longitude/latitude CRS.

use arrow_schema::Field;
use datafusion::common::plan_datafusion_err;
use datafusion::error::Result;
use geoarrow_schema::{Edges, Metadata};
use wkt::types::Coord;

use crate::util::field::input_metadata;
use crate::util::srid::{
    SRID_GEOGRAPHY_DEFAULT, SRID_UNKNOWN, Spheroid, crs_to_srid, lonlat_spheroid, srid_to_crs,
};

/// Whether a field holds geography: GeoArrow's spherical edges are PostGIS's great-circle arcs.
pub(crate) fn is_geography(field: &Field) -> bool {
    input_metadata(field).edges() == Some(Edges::Spherical)
}

/// The SRID of a geography field. Without a CRS it's 4326, as GeoParquet reads a missing CRS as
/// WGS 84 longitude/latitude. `None` for a CRS that doesn't name an SRID.
pub(crate) fn geography_srid(field: &Field) -> Option<i32> {
    match crs_to_srid(input_metadata(field).crs())? {
        SRID_UNKNOWN => Some(SRID_GEOGRAPHY_DEFAULT),
        srid => Some(srid),
    }
}

/// The metadata of geography with `srid`: unknown becomes 4326, and any other SRID than a
/// longitude/latitude one is an error, as in PostGIS.
pub(crate) fn geography_metadata(name: &str, srid: i32) -> Result<Metadata> {
    let srid = match srid {
        SRID_UNKNOWN => SRID_GEOGRAPHY_DEFAULT,
        srid => srid,
    };
    geography_spheroid(name, srid)?;
    Ok(Metadata::new(srid_to_crs(srid), Some(Edges::Spherical)))
}

/// The spheroid of a geography SRID, or PostGIS's error for one that isn't longitude/latitude.
pub(crate) fn geography_spheroid(name: &str, srid: i32) -> Result<Spheroid> {
    lonlat_spheroid(srid).ok_or_else(|| {
        plan_datafusion_err!(
            "{name}: Only lon/lat coordinate systems are supported in geography (SRID {srid})."
        )
    })
}

/// The coordinate with its longitude and latitude brought into range, as PostGIS does for
/// geography input. A coordinate out of range has its longitude moved by whole turns into
/// (-180, 180], so -180 becomes 180, and its latitude too, after which one beyond a pole is folded
/// back over it (100 becomes 80); the longitude stays. Z and M are kept.
///
/// The steps are those PostGIS's results show to the last digit: a remainder of a full turn
/// beyond one, then one turn added or taken away.
pub(crate) fn coerce_into_range(coord: Coord<f64>) -> Coord<f64> {
    if (-180.0..=180.0).contains(&coord.x) && (-90.0..=90.0).contains(&coord.y) {
        return coord;
    }
    let x = match wrap(coord.x) {
        -180.0 => 180.0,
        x => x,
    };
    let y = match wrap(coord.y) {
        y if y > 90.0 => 180.0 - y,
        y if y < -90.0 => -180.0 - y,
        y => y,
    };
    Coord { x, y, ..coord }
}

/// The angle moved by whole turns into [-180, 180]. Infinity and NaN give NaN.
fn wrap(mut degrees: f64) -> f64 {
    if degrees.abs() > 360.0 {
        degrees %= 360.0;
    }
    if degrees > 180.0 {
        degrees - 360.0
    } else if degrees < -180.0 {
        degrees + 360.0
    } else {
        degrees
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn coerce(x: f64, y: f64) -> (f64, f64) {
        let coord = coerce_into_range(Coord {
            x,
            y,
            z: None,
            m: None,
        });
        (coord.x, coord.y)
    }

    #[test]
    fn test_coerce_into_range_matches_postgis() {
        // Expected values from PostGIS 3.6.4: ST_AsText(ST_MakePoint(x, y)::geography).
        for (x, expected) in [
            (181.0, -179.0),
            (360.0, 0.0),
            (540.0, 180.0),
            (-181.0, 179.0),
            (-180.0, -180.0),
            (-540.0, 180.0),
            (-900.0, 180.0),
            (720.5, 0.5),
            (1e10, -80.0),
            (-1e10, 80.0),
        ] {
            assert_eq!(coerce(x, 0.0).0, expected, "x = {x}");
        }
        for (y, expected) in [
            (91.0, 89.0),
            (180.0, 0.0),
            (270.0, -90.0),
            (450.0, 90.0),
            (1000.0, -80.0),
            (-91.0, -89.0),
            (-270.0, 90.0),
            (-200.0, 20.0),
            (200.0, -20.0),
        ] {
            assert_eq!(coerce(0.0, y).1, expected, "y = {y}");
        }
        assert_eq!(coerce(200.0, 100.0), (-160.0, 80.0));
        // A longitude of -180 in a coordinate out of range becomes 180.
        assert_eq!(coerce(-180.0, 190.0), (180.0, -10.0));
        assert!(coerce(f64::INFINITY, 0.0).0.is_nan());
    }
}
