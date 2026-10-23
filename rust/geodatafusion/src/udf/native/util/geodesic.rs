//! Measurement on the spheroid, or the sphere, of a longitude/latitude SRID: what PostGIS's
//! geography forms compute.
//!
//! On the spheroid, distances, azimuths and areas are GeographicLib's geodesics. On the sphere,
//! of PostGIS's mean radius (2a + b) / 3, distances are Vincenty's formula, also for a spheroid
//! that is a sphere. For distances and azimuths PostGIS turns coordinates into radians
//! (`x * PI / 180`) and, for GeographicLib, back into degrees (`x * 180 / PI`), which moves some
//! by an ulp; its results show it, so the same happens here.

use std::f64::consts::PI;
use std::sync::Arc;

use arrow_array::{Array, BooleanArray};
use arrow_schema::FieldRef;
use datafusion::common::exec_datafusion_err;
use datafusion::logical_expr::{ColumnarValue, ScalarFunctionArgs};
use geo_traits::{CoordTrait, GeometryTrait, LineStringTrait, PolygonTrait};
use geoarrow_array::GeoArrowArray;
use geographiclib_rs::{InverseGeodesic, PolygonArea, Winding};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodetic_distance::{Nearest, nearest};
use crate::util::args::optional_bool_arg;
use crate::util::field::geometries_from_array;
use crate::util::geography::{geography_argument, geography_spheroid, is_geography};
use crate::util::ordinates::z;
use crate::util::scan::scan_number;
use crate::util::srid::Spheroid;

/// A geometry argument to measure as geography, with what to measure it on.
pub(crate) struct GeodeticArgument {
    pub(crate) geometries: Arc<dyn GeoArrowArray>,
    pub(crate) geodesic: Geodesic,
    /// Per row, whether to measure on the spheroid rather than the sphere.
    pub(crate) use_spheroid: BooleanArray,
}

impl GeodeticArgument {
    /// Whether row `row` measures on the spheroid; `None` for a NULL `use_spheroid`.
    pub(crate) fn use_spheroid(&self, row: usize) -> Option<bool> {
        (!self.use_spheroid.is_null(row)).then(|| self.use_spheroid.value(row))
    }
}

/// The two geometry arguments of a measurement function, to measure as geography.
pub(crate) struct GeodeticPair {
    pub(crate) first: GeodeticArgument,
    pub(crate) second: (ColumnarValue, FieldRef),
}

/// Arguments 0 and 1 of a measurement function, to measure as geography if either is
/// geography, or if the call has the geography form's `use_spheroid` argument (`flag`). The
/// other is cast to geography, as PostgreSQL does; their SRIDs must match. `None` to measure in
/// the plane.
pub(crate) fn geodetic_pair(
    name: &str,
    args: &ScalarFunctionArgs,
    flag: Option<usize>,
) -> GeoDataFusionResult<Option<GeodeticPair>> {
    let flagged = flag.is_some_and(|flag| args.args.len() > flag);
    if !flagged && !args.arg_fields[..2].iter().any(|field| is_geography(field)) {
        return Ok(None);
    }
    let use_spheroid = match flag {
        Some(flag) => optional_bool_arg(args, flag, true)?,
        None => BooleanArray::from(vec![true; args.number_rows]),
    };
    geography_pair(name, args, use_spheroid).map(Some)
}

/// Arguments 0 and 1, cast to geography as PostgreSQL does, to measure as `use_spheroid` says.
/// Their SRIDs must match.
pub(crate) fn geography_pair(
    name: &str,
    args: &ScalarFunctionArgs,
    use_spheroid: BooleanArray,
) -> GeoDataFusionResult<GeodeticPair> {
    let (first, first_field, srid) = geography_argument(name, args, 0)?;
    let (second, second_field, second_srid) = geography_argument(name, args, 1)?;
    if srid != second_srid {
        return Err(exec_datafusion_err!(
            "{name}: Operation on mixed SRID geometries ({srid} != {second_srid})"
        )
        .into());
    }
    Ok(GeodeticPair {
        first: GeodeticArgument {
            geometries: geometries_from_array(&first.to_array(args.number_rows)?, &first_field)?,
            geodesic: Geodesic::new(geography_spheroid(name, srid)?),
            use_spheroid,
        },
        second: (second, second_field),
    })
}

/// Argument `index` of a measurement function, to measure as geography if it is geography, or
/// if the call has the geography form's `use_spheroid` argument (`flag`): PostgreSQL casts
/// geometry to geography for that form. `None` to measure in the plane.
pub(crate) fn geodetic_argument(
    name: &str,
    args: &ScalarFunctionArgs,
    index: usize,
    flag: usize,
) -> GeoDataFusionResult<Option<GeodeticArgument>> {
    if !is_geography(&args.arg_fields[index]) && args.args.len() <= flag {
        return Ok(None);
    }
    let (value, field, srid) = geography_argument(name, args, index)?;
    let geometries = geometries_from_array(&value.to_array(args.number_rows)?, &field)?;
    Ok(Some(GeodeticArgument {
        geometries,
        geodesic: Geodesic::new(geography_spheroid(name, srid)?),
        use_spheroid: optional_bool_arg(args, flag, true)?,
    }))
}

/// The geodesics of one spheroid.
pub(crate) struct Geodesic {
    geodesic: geographiclib_rs::Geodesic,
    /// The radius of the sphere PostGIS measures on without the spheroid.
    radius: f64,
    /// Whether the spheroid is a sphere, which PostGIS measures on as such.
    sphere: bool,
}

impl Geodesic {
    pub(crate) fn new(spheroid: Spheroid) -> Self {
        let Spheroid { a, b } = spheroid;
        Self::with_flattening(a, (a - b) / a)
    }

    /// The geodesics of the spheroid with semi-major axis `a` and flattening `f`.
    pub(crate) fn with_flattening(a: f64, f: f64) -> Self {
        let b = a - a * f;
        Self {
            geodesic: geographiclib_rs::Geodesic::new(a, f),
            radius: (2.0 * a + b) / 3.0,
            sphere: a == b,
        }
    }

    /// The distance in metres between two points, given as longitude and latitude in degrees:
    /// along the geodesic, or the great circle if `use_spheroid` is false.
    pub(crate) fn distance(&self, p: (f64, f64), q: (f64, f64), use_spheroid: bool) -> f64 {
        let radians = |(lon, lat): (f64, f64)| (radians(lon), radians(lat));
        self.distance_radians(radians(p), radians(q), use_spheroid)
    }

    /// The distance in metres between two points given as longitude and latitude in radians,
    /// which GeographicLib gets in degrees.
    fn distance_radians(&self, p: (f64, f64), q: (f64, f64), use_spheroid: bool) -> f64 {
        if use_spheroid && !self.sphere {
            self.geodesic
                .inverse(degrees(p.1), degrees(p.0), degrees(q.1), degrees(q.0))
        } else {
            self.radius * central_angle(p, q)
        }
    }

    /// The azimuth of the geodesic from `p` to `q` at `p`, in radians clockwise from north in
    /// [0, 2π).
    pub(crate) fn azimuth(&self, p: (f64, f64), q: (f64, f64)) -> f64 {
        let (azimuth, _, _): (f64, f64, f64) = self.geodesic.inverse(
            round_trip(p.1),
            round_trip(p.0),
            round_trip(q.1),
            round_trip(q.0),
        );
        let azimuth = radians(azimuth);
        if azimuth < 0.0 {
            azimuth + 2.0 * PI
        } else {
            azimuth
        }
    }

    /// The length in metres of a line: the sum of its segments, each with its rise in Z added
    /// by Pythagoras.
    pub(crate) fn length(&self, line: &impl LineStringTrait<T = f64>, use_spheroid: bool) -> f64 {
        let mut coords = line.coords();
        let Some(mut previous) = coords.next() else {
            return 0.0;
        };
        let mut length = 0.0;
        for coord in coords {
            let distance = self.distance(xy(&previous), xy(&coord), use_spheroid);
            length += match (z(&previous), z(&coord)) {
                (Some(z1), Some(z2)) => (distance * distance + (z2 - z1) * (z2 - z1)).sqrt(),
                _ => distance,
            };
            previous = coord;
        }
        length
    }

    /// The distance in metres between the nearest points of two geometries, found on the
    /// sphere and measured as `use_spheroid` says; `None` if either is empty.
    pub(crate) fn geometry_distance(
        &self,
        a: &impl GeometryTrait<T = f64>,
        b: &impl GeometryTrait<T = f64>,
        use_spheroid: bool,
    ) -> Option<f64> {
        let Nearest { points: [p, q], .. } = nearest(a, b)?;
        Some(self.distance_radians((p.lon, p.lat), (q.lon, q.lat), use_spheroid))
    }

    /// The angle at the centre of the sphere between the nearest points of two geometries;
    /// `None` if either is empty.
    pub(crate) fn geometry_angle(
        &self,
        a: &impl GeometryTrait<T = f64>,
        b: &impl GeometryTrait<T = f64>,
    ) -> Option<f64> {
        let Nearest { points: [p, q], .. } = nearest(a, b)?;
        Some(central_angle((p.lon, p.lat), (q.lon, q.lat)))
    }

    /// The length in metres of a polygon's rings, its holes' included.
    pub(crate) fn perimeter(
        &self,
        polygon: &impl PolygonTrait<T = f64>,
        use_spheroid: bool,
    ) -> f64 {
        let mut perimeter = 0.0;
        for ring in polygon.exterior().into_iter().chain(polygon.interiors()) {
            perimeter += self.length(&ring, use_spheroid);
        }
        perimeter
    }

    /// The area in square metres of a polygon: its shell's less its holes'. A negative area,
    /// of holes larger than their shell, is `None`.
    pub(crate) fn area(&self, polygon: &impl PolygonTrait<T = f64>) -> Option<f64> {
        let Some(shell) = polygon.exterior() else {
            return Some(0.0);
        };
        let area = polygon
            .interiors()
            .fold(self.ring_area(&shell), |area, hole| {
                area - self.ring_area(&hole)
            });
        (area >= 0.0).then_some(area)
    }

    /// The area in square metres inside a ring, on the spheroid, whichever way it turns.
    fn ring_area(&self, ring: &impl LineStringTrait<T = f64>) -> f64 {
        let mut polygon = PolygonArea::new(&self.geodesic, Winding::CounterClockwise);
        // The polygon closes itself, so the ring's closing point is left out.
        let count = ring.num_coords().saturating_sub(1);
        // Unlike distances, areas take the degrees as they are.
        for coord in ring.coords().take(count) {
            polygon.add_point(coord.y(), coord.x());
        }
        let (_, area, _) = polygon.compute(true);
        area.abs()
    }
}

/// PostGIS's spheroid text, `SPHEROID["name",a,rf]` (or with parentheses), as `(a, rf)`, read as
/// PostGIS reads it: the name has 1 to 19 characters, each number ends at the comma after it,
/// whitespace may come before a number, and text after the second number is ignored.
pub(crate) fn parse_spheroid(text: &str) -> Result<(f64, f64), &'static str> {
    const BAD: &str = "SPHEROID parser - couldn't parse the spheroid";
    let Some(rest) = text.strip_prefix("SPHEROID") else {
        return Err("SPHEROID parser - doesn't start with SPHEROID");
    };
    let rest = rest
        .strip_prefix("[\"")
        .or_else(|| rest.strip_prefix("(\""))
        .ok_or(BAD)?;
    let (name, rest) = rest.split_once('"').ok_or(BAD)?;
    if name.is_empty() || name.chars().count() > 19 {
        return Err(BAD);
    }
    let rest = rest.strip_prefix(',').ok_or(BAD)?;
    let (a, rest) = scan_number(rest).ok_or(BAD)?;
    let rest = rest.strip_prefix(',').ok_or(BAD)?;
    let (rf, _) = scan_number(rest).ok_or(BAD)?;
    Ok((a, rf))
}

pub(crate) fn xy(coord: &impl CoordTrait<T = f64>) -> (f64, f64) {
    (coord.x(), coord.y())
}

fn radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

fn degrees(radians: f64) -> f64 {
    radians * 180.0 / PI
}

fn round_trip(degrees: f64) -> f64 {
    self::degrees(radians(degrees))
}

/// The angle between two points, as longitude and latitude in radians, at the centre of the
/// unit sphere, by Vincenty's formula.
fn central_angle(p: (f64, f64), q: (f64, f64)) -> f64 {
    let (lat1, lat2) = (p.1, q.1);
    let delta = q.0 - p.0;
    let (sin1, cos1) = lat1.sin_cos();
    let (sin2, cos2) = lat2.sin_cos();
    let (sin_delta, cos_delta) = delta.sin_cos();
    let a = cos2 * sin_delta;
    let b = cos1 * sin2 - sin1 * cos2 * cos_delta;
    let y = (a * a + b * b).sqrt();
    let x = sin1 * sin2 + cos1 * cos2 * cos_delta;
    y.atan2(x)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_spheroid() {
        let wgs84 = Ok((6378137.0, 298.257223563));
        assert_eq!(
            parse_spheroid(r#"SPHEROID["WGS 84",6378137,298.257223563]"#),
            wgs84
        );
        assert_eq!(
            parse_spheroid(r#"SPHEROID("x", 6378137, 298.257223563]"#),
            wgs84
        );
        assert_eq!(
            parse_spheroid(r#"SPHEROID["x",6378137,298.257223563, 5]"#),
            wgs84
        );
        assert_eq!(
            parse_spheroid(r#"SPHEROID["x",6.378137e6,298.257223563"#),
            wgs84
        );
        for bad in [
            r#"spheroid["x",6378137,298.257223563]"#,
            r#"SPHEROID ["x",6378137,298.257223563]"#,
            r#"SPHEROID["",6378137,298.257223563]"#,
            r#"SPHEROID["12345678901234567890",6378137,298.257223563]"#,
            r#"SPHEROID["x",6378137 ,298.257223563]"#,
            r#"SPHEROID["x",6378137]"#,
        ] {
            assert!(parse_spheroid(bad).is_err(), "{bad}");
        }
    }
}
