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
use geo_traits::{
    CoordTrait, GeometryTrait, GeometryType, LineStringTrait, MultiLineStringTrait,
    MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};
use geoarrow_array::GeoArrowArray;
use geographiclib_rs::{DirectGeodesic, InverseGeodesic, PolygonArea, Winding};

use crate::error::GeoDataFusionResult;
use crate::udf::native::util::geodetic_distance::{
    Nearest, Spot, Vector, coincide, nearest, norm, normalize,
};
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
    /// The spheroid with the semi-major axis of the sphere's radius, on which PostGIS measures
    /// areas without the spheroid.
    sphere_area_geodesic: geographiclib_rs::Geodesic,
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
        let radius = (2.0 * a + b) / 3.0;
        Self {
            geodesic: geographiclib_rs::Geodesic::new(a, f),
            sphere_area_geodesic: geographiclib_rs::Geodesic::new(radius, f),
            radius,
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

    /// The point `distance` metres from `p` along the geodesic leaving it at `azimuth`, in
    /// radians clockwise from north, as PostGIS projects: a negative distance goes the other way,
    /// and the azimuth, normalised to [0, 2π), and the coordinates go through radians and back.
    /// `None` for a distance beyond half the sphere's circumference, which PostGIS refuses.
    pub(crate) fn project(&self, p: (f64, f64), azimuth: f64, distance: f64) -> Option<(f64, f64)> {
        let (azimuth, distance) = if distance < 0.0 {
            (azimuth + PI, -distance)
        } else {
            (azimuth, distance)
        };
        if distance > PI * self.radius {
            return None;
        }
        let azimuth = degrees(azimuth.rem_euclid(2.0 * PI));
        let (lat, lon): (f64, f64) =
            self.geodesic
                .direct(round_trip(p.1), round_trip(p.0), azimuth, distance);
        Some((round_trip(lon), round_trip(lat)))
    }

    /// The point `distance` metres from `p` towards `q`: along the geodesic on the spheroid, or
    /// along the great circle on the sphere, each from `p`'s azimuth, as PostGIS's geography
    /// linear referencing goes.
    pub(crate) fn along(
        &self,
        p: (f64, f64),
        q: (f64, f64),
        distance: f64,
        use_spheroid: bool,
    ) -> (f64, f64) {
        if use_spheroid && !self.sphere {
            let azimuth = self.azimuth(p, q);
            return self
                .project(p, azimuth, distance)
                .unwrap_or((f64::NAN, f64::NAN));
        }
        let (lon1, lat1) = (radians(p.0), radians(p.1));
        let (lon2, lat2) = (radians(q.0), radians(q.1));
        // The initial bearing on the sphere, then the destination by it.
        let delta = lon2 - lon1;
        let bearing = (delta.sin() * lat2.cos())
            .atan2(lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * delta.cos());
        let angle = distance / self.radius;
        let lat = (lat1.sin() * angle.cos() + lat1.cos() * angle.sin() * bearing.cos()).asin();
        let mut lon = lon1
            + (bearing.sin() * angle.sin() * lat1.cos())
                .atan2(angle.cos() - lat1.sin() * lat.sin());
        // Back into (-180, 180], as PostGIS normalises.
        if lon > PI {
            lon -= 2.0 * PI;
        } else if lon <= -PI {
            lon += 2.0 * PI;
        }
        (degrees(lon), degrees(lat))
    }

    /// The radius of the sphere PostGIS measures on without the spheroid.
    pub(crate) fn radius(&self) -> f64 {
        self.radius
    }

    /// Half the circumference of the sphere, the farthest PostGIS projects.
    pub(crate) fn half_circumference(&self) -> f64 {
        PI * self.radius
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
        if coincide(&p, &q) {
            return Some(0.0);
        }
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

    /// The area in square metres of a polygon: its shell's less its holes', so negative for
    /// holes larger than their shell.
    ///
    /// Without `use_spheroid`, PostGIS's results are the areas on a spheroid of the same
    /// flattening with the sphere's radius for semi-major axis, (R / a)² times those on the
    /// spheroid, rather than areas on the sphere; so they are here.
    pub(crate) fn area(&self, polygon: &impl PolygonTrait<T = f64>, use_spheroid: bool) -> f64 {
        let geodesic = if use_spheroid {
            &self.geodesic
        } else {
            &self.sphere_area_geodesic
        };
        let Some(shell) = polygon.exterior() else {
            return 0.0;
        };
        polygon
            .interiors()
            .fold(ring_area(geodesic, &shell), |area, hole| {
                area - ring_area(geodesic, &hole)
            })
    }
}

/// The area in square metres inside a ring, whichever way it turns.
fn ring_area(geodesic: &geographiclib_rs::Geodesic, ring: &impl LineStringTrait<T = f64>) -> f64 {
    let mut polygon = PolygonArea::new(geodesic, Winding::CounterClockwise);
    // The polygon closes itself, so the ring's closing point is left out.
    let count = ring.num_coords().saturating_sub(1);
    // Unlike distances, areas take the degrees as they are.
    for coord in ring.coords().take(count) {
        polygon.add_point(coord.y(), coord.x());
    }
    let (_, area, _) = polygon.compute(true);
    area.abs()
}

#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_Centroid, backed by GEOS, takes geography centroids"
    )
)]
impl Geodesic {
    /// The centroid of geography, as PostGIS computes it, as longitude and latitude in degrees;
    /// `Ok(None)` if it has no points, and PostGIS's error for a collection.
    ///
    /// A point is its own centroid; points are averaged as unit vectors. Lines sum the two ends of
    /// each segment, weighted by its length. Polygons sum unit-vector triangle centroids, weighted
    /// by the triangles' unsigned areas: triangles from the first polygon's first point to every
    /// edge of every ring, holes included, so holes add to the weight. Zero weights give NaN. Lengths and areas are on the spheroid, or with `use_spheroid`
    /// false on the sphere, where triangle areas are spherical excess.
    pub(crate) fn centroid(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        use_spheroid: bool,
    ) -> Result<Option<(f64, f64)>, &'static str> {
        let mut sum = [0.0; 3];
        let mut any = false;
        let mut add = |vector: Vector, weight: f64| {
            any = true;
            for (total, value) in sum.iter_mut().zip(vector) {
                *total += weight * value;
            }
        };
        let vector =
            |coord: &dyn CoordTrait<T = f64>| Spot::from_degrees(coord.x(), coord.y()).vector();
        match geom.as_type() {
            // A point is its own centroid.
            GeometryType::Point(point) => return Ok(point.coord().map(|coord| xy(&coord))),
            GeometryType::MultiPoint(points) => {
                for point in points.points() {
                    if let Some(coord) = point.coord() {
                        add(vector(&coord), 1.0);
                    }
                }
            }
            GeometryType::LineString(line) => self.add_line(&mut add, line, use_spheroid),
            GeometryType::MultiLineString(lines) => {
                for line in lines.line_strings() {
                    self.add_line(&mut add, &line, use_spheroid);
                }
            }
            GeometryType::Polygon(polygon) => {
                if let Some(origin) = shell_start(polygon) {
                    self.add_polygon(&mut add, polygon, origin, use_spheroid);
                }
            }
            // Every polygon's triangles start at the first polygon's first point.
            GeometryType::MultiPolygon(polygons) => {
                if let Some(origin) = polygons
                    .polygons()
                    .find_map(|polygon| shell_start(&polygon))
                {
                    for polygon in polygons.polygons() {
                        self.add_polygon(&mut add, &polygon, origin, use_spheroid);
                    }
                }
            }
            _ => return Err("ST_Centroid(geography) unhandled geography type"),
        }
        if !any {
            return Ok(None);
        }
        // Zero weights, of segments or triangles without length or area, leave PostGIS dividing
        // by zero: NaN.
        let length = norm(&sum);
        if length == 0.0 {
            return Ok(Some((f64::NAN, f64::NAN)));
        }
        let centroid = Spot::from_vector(normalize(&sum, length));
        Ok(Some((degrees(centroid.lon), degrees(centroid.lat))))
    }

    fn add_line(
        &self,
        add: &mut impl FnMut(Vector, f64),
        line: &impl LineStringTrait<T = f64>,
        use_spheroid: bool,
    ) {
        let coords: Vec<(f64, f64)> = line.coords().map(|coord| xy(&coord)).collect();
        for pair in coords.windows(2) {
            let (a, b) = (
                Spot::from_degrees(pair[0].0, pair[0].1).vector(),
                Spot::from_degrees(pair[1].0, pair[1].1).vector(),
            );
            let middle = [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
            add(middle, self.distance(pair[0], pair[1], use_spheroid));
        }
    }

    fn add_polygon(
        &self,
        add: &mut impl FnMut(Vector, f64),
        polygon: &impl PolygonTrait<T = f64>,
        origin: (f64, f64),
        use_spheroid: bool,
    ) {
        let o = Spot::from_degrees(origin.0, origin.1);
        for ring in polygon.exterior().into_iter().chain(polygon.interiors()) {
            let coords: Vec<(f64, f64)> = ring.coords().map(|coord| xy(&coord)).collect();
            for pair in coords.windows(2) {
                let (q, r) = (
                    Spot::from_degrees(pair[0].0, pair[0].1),
                    Spot::from_degrees(pair[1].0, pair[1].1),
                );
                let (v0, v1, v2) = (o.vector(), q.vector(), r.vector());
                let total = [
                    v0[0] + v1[0] + v2[0],
                    v0[1] + v1[1] + v2[1],
                    v0[2] + v1[2] + v2[2],
                ];
                let length = norm(&total);
                if length == 0.0 {
                    continue;
                }
                let area = if use_spheroid {
                    self.triangle_area([origin, pair[0], pair[1]])
                } else {
                    spherical_excess([o, q, r])
                };
                add(normalize(&total, length), area.abs());
            }
        }
    }

    /// The signed area of a triangle on the spheroid, its corners in degrees.
    fn triangle_area(&self, corners: [(f64, f64); 3]) -> f64 {
        let mut polygon = PolygonArea::new(&self.geodesic, Winding::CounterClockwise);
        for (lon, lat) in corners {
            polygon.add_point(lat, lon);
        }
        let (_, area, _) = polygon.compute(true);
        area
    }
}

#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_Centroid, backed by GEOS, takes geography centroids"
    )
)]
/// The spherical excess of a triangle on the unit sphere, signed by its turn: the sum over its
/// edges of the area between the edge and the equator, by the tangent half-angle formula.
fn spherical_excess(corners: [Spot; 3]) -> f64 {
    let mut excess = 0.0;
    for (p, q) in [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[0]),
    ] {
        let (t1, t2) = ((p.lat / 2.0).tan(), (q.lat / 2.0).tan());
        excess += 2.0 * (((q.lon - p.lon) / 2.0).tan() * (t1 + t2)).atan2(1.0 + t1 * t2);
    }
    excess
}

/// The first point of a polygon's shell, if it has one.
#[cfg_attr(
    not(feature = "geos-3_11"),
    expect(
        dead_code,
        reason = "only ST_Centroid, backed by GEOS, takes geography centroids"
    )
)]
fn shell_start(polygon: &impl PolygonTrait<T = f64>) -> Option<(f64, f64)> {
    polygon.exterior()?.coords().next().map(|coord| xy(&coord))
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
