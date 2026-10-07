//! GeoHash encoding and decoding, matching PostGIS.
//!
//! A GeoHash bisects the longitude range [-180, 180] and the latitude range [-90, 90] in turn,
//! starting with longitude, one bit per step, and writes every five bits as one base32
//! character. The rules for the length, the boundaries and the decoded centre were derived
//! from PostGIS's output and are checked against it by the test data in `testdata/geohash.txt`.

use std::fmt;

use wkt::Wkt;
use wkt::types::{Coord, LineString, Point, Polygon};

/// The GeoHash alphabet: the digits and the lowercase letters except a, i, l and o.
const BASE32: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// The length of a point's GeoHash when no length is given.
const POINT_LENGTH: usize = 20;

/// A box as `[xmin, ymin, xmax, ymax]`.
pub(crate) type Bounds = [f64; 4];

const WORLD: Bounds = [-180.0, -90.0, 180.0, 90.0];

/// Why a GeoHash can't be written or read. The messages are PostGIS's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeoHashError {
    /// The box is outside longitude/latitude bounds.
    OutOfRange(Bounds),
    /// The hash has a character outside the alphabet.
    InvalidCharacter(char),
}

impl fmt::Display for GeoHashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoHashError::OutOfRange([xmin, ymin, xmax, ymax]) => write!(
                f,
                "Geohash requires inputs in decimal degrees, got ({xmin} {ymin}, {xmax} {ymax})."
            ),
            GeoHashError::InvalidCharacter(c) => write!(f, "Invalid character '{c}'"),
        }
    }
}

/// Writes the GeoHash of a box (a point is a box with no extent).
///
/// With `max_chars > 0` it's the centre's GeoHash with that many characters. Otherwise it's 20
/// characters for a point, and for any other box the longest GeoHash whose cell contains the
/// whole box, which is empty when the box straddles the first bisection.
pub(crate) fn encode(
    out: &mut String,
    bounds: &Bounds,
    max_chars: i32,
) -> Result<(), GeoHashError> {
    let [xmin, ymin, xmax, ymax] = *bounds;
    if xmin < WORLD[0] || ymin < WORLD[1] || xmax > WORLD[2] || ymax > WORLD[3] {
        return Err(GeoHashError::OutOfRange(*bounds));
    }
    let chars = match usize::try_from(max_chars) {
        Ok(chars) if chars > 0 => chars,
        _ if xmin == xmax && ymin == ymax => POINT_LENGTH,
        _ => precision(bounds),
    };
    encode_point(out, (xmin + xmax) / 2.0, (ymin + ymax) / 2.0, chars);
    Ok(())
}

/// Writes `chars` characters of a point's GeoHash. A coordinate on a bisection goes to the
/// upper half.
fn encode_point(out: &mut String, x: f64, y: f64, chars: usize) {
    let (mut x_range, mut y_range) = ([WORLD[0], WORLD[2]], [WORLD[1], WORLD[3]]);
    let mut is_x = true;
    for _ in 0..chars {
        let mut index = 0;
        for _ in 0..5 {
            let (value, range) = if is_x {
                (x, &mut x_range)
            } else {
                (y, &mut y_range)
            };
            let mid = (range[0] + range[1]) / 2.0;
            index <<= 1;
            if value >= mid {
                index |= 1;
                range[0] = mid;
            } else {
                range[1] = mid;
            }
            is_x = !is_x;
        }
        out.push(char::from(BASE32[index]));
    }
}

/// The number of characters of the longest GeoHash whose cell contains the whole box: the
/// number of whole characters of bisections the box doesn't straddle.
fn precision(bounds: &Bounds) -> usize {
    let [xmin, ymin, xmax, ymax] = *bounds;
    let (mut x_range, mut y_range) = ([WORLD[0], WORLD[2]], [WORLD[1], WORLD[3]]);
    let mut bits = 0;
    loop {
        let (min, max, range) = if bits % 2 == 0 {
            (xmin, xmax, &mut x_range)
        } else {
            (ymin, ymax, &mut y_range)
        };
        let mid = (range[0] + range[1]) / 2.0;
        if min > mid {
            range[0] = mid;
        } else if max < mid {
            range[1] = mid;
        } else {
            return bits / 5;
        }
        bits += 1;
    }
}

/// The cell of a GeoHash, read to `precision` characters. A NULL or negative precision, or one
/// longer than the hash, reads the whole hash; 0 reads none, which is the whole world.
/// Uppercase letters are accepted.
pub(crate) fn decode(hash: &str, precision: Option<i32>) -> Result<Bounds, GeoHashError> {
    let chars = match precision.map(usize::try_from) {
        Some(Ok(precision)) => precision.min(hash.len()),
        _ => hash.len(),
    };
    let (mut x_range, mut y_range) = ([WORLD[0], WORLD[2]], [WORLD[1], WORLD[3]]);
    let mut is_x = true;
    for c in hash.chars().take(chars) {
        let lower = c.to_ascii_lowercase();
        let index = BASE32
            .iter()
            .position(|&b| char::from(b) == lower)
            .ok_or(GeoHashError::InvalidCharacter(c))?;
        for bit in (0..5).rev() {
            let range = if is_x { &mut x_range } else { &mut y_range };
            let mid = (range[0] + range[1]) / 2.0;
            if index & (1 << bit) != 0 {
                range[0] = mid;
            } else {
                range[1] = mid;
            }
            is_x = !is_x;
        }
    }
    Ok([x_range[0], y_range[0], x_range[1], y_range[1]])
}

/// The centre of a cell, the point PostGIS decodes a GeoHash to.
pub(crate) fn center(bounds: &Bounds) -> (f64, f64) {
    let [xmin, ymin, xmax, ymax] = *bounds;
    (xmin + (xmax - xmin) / 2.0, ymin + (ymax - ymin) / 2.0)
}

/// A cell as a geometry: a polygon, or a point or a line where the cell collapsed in floating
/// point, as PostGIS's ST_GeomFromGeoHash returns.
pub(crate) fn cell_geometry(bounds: &Bounds) -> Wkt<f64> {
    let [xmin, ymin, xmax, ymax] = *bounds;
    let coord = |x, y| Coord {
        x,
        y,
        z: None,
        m: None,
    };
    match (xmin == xmax, ymin == ymax) {
        (true, true) => Wkt::Point(Point::from_coord(coord(xmin, ymin))),
        (true, false) | (false, true) => Wkt::LineString(
            LineString::from_coords([coord(xmin, ymin), coord(xmax, ymax)])
                .expect("two XY coordinates make a line"),
        ),
        (false, false) => {
            let ring = LineString::from_coords([
                coord(xmin, ymin),
                coord(xmin, ymax),
                coord(xmax, ymax),
                coord(xmax, ymin),
                coord(xmin, ymin),
            ])
            .expect("five XY coordinates make a ring");
            Wkt::Polygon(Polygon::from_rings([ring]).expect("one XY ring makes a polygon"))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::udf::native::io::util::wkt::{WktFlavor, write_wkt};

    fn coord(x: f64, y: f64) -> Coord {
        Coord {
            x,
            y,
            z: None,
            m: None,
        }
    }

    fn parse_pair(text: &str) -> (f64, f64) {
        let (x, y) = text.split_once(' ').unwrap();
        (x.parse().unwrap(), y.parse().unwrap())
    }

    #[test]
    fn test_encode_matches_postgis() {
        let cases = include_str!("testdata/geohash.txt");
        let mut mismatches = vec![];
        for line in cases.lines().filter(|line| line.starts_with("encode|")) {
            let fields: Vec<&str> = line.split('|').collect();
            let [_, min, max, max_chars, expected] = fields[..] else {
                panic!("malformed test case {line:?}");
            };
            let ((xmin, ymin), (xmax, ymax)) = (parse_pair(min), parse_pair(max));
            let mut actual = String::new();
            let actual = match encode(
                &mut actual,
                &[xmin, ymin, xmax, ymax],
                max_chars.parse().unwrap(),
            ) {
                Err(_) => "ERROR".to_string(),
                Ok(()) if actual.is_empty() => "(empty)".to_string(),
                Ok(()) => actual,
            };
            if actual != expected {
                mismatches.push(format!("{line}: got {actual}"));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} mismatches:\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }

    #[test]
    fn test_decode_matches_postgis() {
        let cases = include_str!("testdata/geohash.txt");
        let mut mismatches = vec![];
        for line in cases.lines().filter(|line| line.starts_with("decode|")) {
            let fields: Vec<&str> = line.split('|').collect();
            let [_, hash, precision, expected_point, expected_polygon] = fields[..] else {
                panic!("malformed test case {line:?}");
            };
            let precision = (precision != "NULL").then(|| precision.parse().unwrap());
            let bounds = decode(hash, precision).unwrap();
            let (x, y) = center(&bounds);
            let mut point = String::new();
            write_wkt(
                &mut point,
                &Point::from_coord(coord(x, y)),
                WktFlavor::Iso,
                15,
            );
            let mut polygon = String::new();
            write_wkt(&mut polygon, &cell_geometry(&bounds), WktFlavor::Iso, 15);
            if point != expected_point || polygon != expected_polygon {
                mismatches.push(format!("{line}: got {point}|{polygon}"));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} mismatches:\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }

    #[test]
    fn test_invalid_character() {
        assert_eq!(
            decode("9qqa", None),
            Err(GeoHashError::InvalidCharacter('a'))
        );
    }
}
