//! PostGIS's choice of a planar projection for geography, `_ST_BestSRID`, and the projections
//! of the SRIDs it chooses from, which PostGIS defines internally rather than in
//! `spatial_ref_sys`.
//!
//! The choice is made from the geocentric box as PostGIS stores it, rounded outward to single
//! precision (see [`GeocentricBox`]), through its eight corners: its centre is the mean of the
//! corners as unit vectors, its width the largest angle between two corners seen down the
//! z-axis, and its height the range of the corners' latitudes. In order:
//!
//! - North or south of 70° of latitude, and under 45° high: Lambert azimuthal equal-area on
//!   the pole, 999061 or 999161.
//! - Under 6° wide: the UTM zone of the centre, 999001 to 999060 north of the equator and
//!   999101 to 999160 south of it.
//! - Under 25° high and narrower than the tiles of its band: a Lambert azimuthal equal-area
//!   tile centred on its band and column. The bands are 30° of latitude, from the south pole,
//!   with tiles 90° wide in the polar bands, 45° in the temperate and 30° in the tropical, from
//!   180° west: SRID 999163 + 20 × band + column.
//! - Otherwise World Mercator, 999000.

use crate::udf::native::util::geocentric_box::GeocentricBox;

/// World Mercator, PostGIS's last resort.
const WORLD_MERCATOR: i32 = 999000;
const POLAR_NORTH: i32 = 999061;
const POLAR_SOUTH: i32 = 999161;
const UTM_NORTH: i32 = 999000;
const UTM_SOUTH: i32 = 999100;
const TILES: i32 = 999163;

/// The SRID PostGIS chooses for geography in `bounds`. An EMPTY geography, with no box, gets
/// UTM zone 31 north; PostGIS's result for it varies from call to call.
pub(crate) fn best_srid(bounds: Option<GeocentricBox>) -> i32 {
    let Some(bounds) = bounds else {
        return UTM_NORTH + 31;
    };
    let [(x0, x1), (y0, y1), (z0, z1)] = bounds.ranges();
    let mut corners = Vec::with_capacity(8);
    for x in [x0, x1] {
        for y in [y0, y1] {
            for z in [z0, z1] {
                corners.push([x, y, z]);
            }
        }
    }
    let mut mean = [0.0; 3];
    for corner in &corners {
        let length = (corner[0] * corner[0] + corner[1] * corner[1] + corner[2] * corner[2]).sqrt();
        for (sum, component) in mean.iter_mut().zip(corner) {
            *sum += component / length;
        }
    }
    let mean = mean.map(|sum| sum / 8.0);
    let lon = mean[1].atan2(mean[0]).to_degrees();
    let lat = mean[2].atan2(mean[0].hypot(mean[1])).to_degrees();
    let latitudes: Vec<f64> = corners
        .iter()
        .map(|[x, y, z]| z.atan2(x.hypot(*y)).to_degrees())
        .collect();
    let height = latitudes.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - latitudes.iter().copied().fold(f64::INFINITY, f64::min);
    let width = width(&corners);

    if height < 45.0 {
        if lat > 70.0 {
            return POLAR_NORTH;
        }
        if lat < -70.0 {
            return POLAR_SOUTH;
        }
    }
    if width < 6.0 {
        let zone = ((lon + 180.0) / 6.0).floor() as i32 + 1;
        return if lat >= 0.0 { UTM_NORTH } else { UTM_SOUTH } + zone;
    }
    if height < 25.0 {
        let band = (((lat + 90.0) / 30.0).floor() as i32).clamp(0, 5);
        let tile_width = tile_width(band);
        if width < tile_width {
            let column = ((lon + 180.0) / tile_width).floor() as i32;
            return TILES + 20 * band + column;
        }
    }
    WORLD_MERCATOR
}

/// The largest angle in degrees between two corners, seen down the z-axis.
fn width(corners: &[[f64; 3]]) -> f64 {
    let mut widest: f64 = 0.0;
    for p in corners {
        for q in corners {
            let cos = (p[0] * q[0] + p[1] * q[1]) / (p[0].hypot(p[1]) * q[0].hypot(q[1]));
            widest = widest.max(cos.clamp(-1.0, 1.0).acos().to_degrees());
        }
    }
    widest
}

/// The width in degrees of the tiles of a band.
fn tile_width(band: i32) -> f64 {
    match band {
        0 | 5 => 90.0,
        1 | 4 => 45.0,
        _ => 30.0,
    }
}

/// The PROJ definition of one of the SRIDs [`best_srid`] chooses; `None` for any other SRID.
#[cfg_attr(
    not(feature = "proj"),
    expect(dead_code, reason = "only ST_Transform, backed by PROJ, reads it")
)]
pub(crate) fn best_srid_projection(srid: i32) -> Option<String> {
    const WGS84: &str = "+ellps=WGS84 +datum=WGS84 +units=m +no_defs";
    Some(match srid {
        WORLD_MERCATOR => format!("+proj=merc +lon_0=0 +k=1 +x_0=0 +y_0=0 {WGS84}"),
        POLAR_NORTH => format!("+proj=laea +lat_0=90 +lon_0=-40 +x_0=0 +y_0=0 {WGS84}"),
        POLAR_SOUTH => format!("+proj=laea +lat_0=-90 +lon_0=0 +x_0=0 +y_0=0 {WGS84}"),
        999001..=999060 => format!("+proj=utm +zone={} {WGS84}", srid - UTM_NORTH),
        999101..=999160 => format!("+proj=utm +zone={} +south {WGS84}", srid - UTM_SOUTH),
        _ => {
            let index = srid.checked_sub(TILES)?;
            let (band, column) = (index / 20, index % 20);
            let tile_width = tile_width(band);
            let columns = (360.0 / tile_width) as i32;
            if !(0..6).contains(&band) || column >= columns {
                return None;
            }
            let lat = -90.0 + 30.0 * band as f64 + 15.0;
            let lon = -180.0 + tile_width * column as f64 + tile_width / 2.0;
            format!("+proj=laea +lat_0={lat} +lon_0={lon} +x_0=0 +y_0=0 {WGS84}")
        }
    })
}
