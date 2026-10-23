//! PostGIS's text form of `box2d` and `box3d`: `BOX(1 2,5 6)` and `BOX3D(1 2 3,4 5 6)`.

use crate::udf::native::bounding_box::util::bounds::BoundingRect;
use crate::udf::native::io::util::number::{DEFAULT_MAX_DECIMAL_DIGITS, write_number};
use crate::util::scan::scan_number;

const BOX2D_ERROR: &str =
    "box2d parser - couldn't parse.  It should look like: BOX(xmin ymin,xmax ymax)";
const BOX3D_ERROR: &str = "BOX3D parser - couldn't parse.  It should look like: BOX3D(xmin ymin zmin,xmax ymax zmax) or BOX3D(xmin ymin,xmax ymax)";

/// Whether text is a `box2d` literal rather than a geometry: it starts with `BOX(`, in any case.
pub(crate) fn is_box2d_text(text: &str) -> bool {
    text.get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("BOX("))
}

/// Whether text is a `box3d` literal: it starts with `BOX3D(`.
pub(crate) fn is_box3d_text(text: &str) -> bool {
    text.starts_with("BOX3D(")
}

/// A `box2d` as PostGIS reads it: `BOX(xmin ymin,xmax ymax)`, the prefix in any case, whitespace
/// allowed before a number but not before the comma, and the closing parenthesis and anything
/// after it ignored. The corners may come in any order.
pub(crate) fn parse_box2d(text: &str) -> Result<BoundingRect, &'static str> {
    if !is_box2d_text(text) {
        return Err(BOX2D_ERROR);
    }
    let corners = scan_corners(&text[4..], 2, false).ok_or(BOX2D_ERROR)?;
    Ok(rect(corners, false))
}

/// A `box3d` as PostGIS reads it: `BOX3D(xmin ymin zmin,xmax ymax zmax)`, or with two numbers a
/// corner for Z 0, whitespace allowed before the numbers and the comma, and the closing
/// parenthesis and anything after it ignored. The corners may come in any order.
pub(crate) fn parse_box3d(text: &str) -> Result<BoundingRect, &'static str> {
    let Some(rest) = text.strip_prefix("BOX3D(") else {
        return Err("BOX3D parser - doesn't start with BOX3D(");
    };
    let corners = scan_corners(rest, 3, true)
        .or_else(|| scan_corners(rest, 2, true))
        .ok_or(BOX3D_ERROR)?;
    Ok(rect(corners, true))
}

/// A box as PostGIS writes a `box2d`: `BOX(xmin ymin,xmax ymax)`, the numbers as in WKT.
pub(crate) fn write_box2d(rect: &BoundingRect) -> String {
    let mut out = String::from("BOX(");
    write_corners(
        &mut out,
        [
            [rect.minx(), rect.miny()].as_slice(),
            &[rect.maxx(), rect.maxy()],
        ],
    );
    out
}

/// A box as PostGIS writes a `box3d`: `BOX3D(xmin ymin zmin,xmax ymax zmax)`, Z 0 if the box has
/// none.
pub(crate) fn write_box3d(rect: &BoundingRect) -> String {
    let mut out = String::from("BOX3D(");
    write_corners(
        &mut out,
        [
            [rect.minx(), rect.miny(), rect.minz()].as_slice(),
            &[rect.maxx(), rect.maxy(), rect.maxz()],
        ],
    );
    out
}

fn write_corners(out: &mut String, corners: [&[f64]; 2]) {
    for (index, corner) in corners.into_iter().enumerate() {
        if index == 1 {
            out.push(',');
        }
        for (index, value) in corner.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            write_number(out, *value, DEFAULT_MAX_DECIMAL_DIGITS);
        }
    }
    out.push(')');
}

/// Two corners of `size` numbers each, separated by a comma.
fn scan_corners(mut text: &str, size: usize, space_before_comma: bool) -> Option<[[f64; 3]; 2]> {
    let mut corners = [[0.0; 3]; 2];
    for (index, corner) in corners.iter_mut().enumerate() {
        if index == 1 {
            if space_before_comma {
                text = text.trim_start();
            }
            text = text.strip_prefix(',')?;
        }
        for value in corner.iter_mut().take(size) {
            let (number, rest) = scan_number(text)?;
            *value = number;
            text = rest;
        }
    }
    Some(corners)
}

/// The box between two corners, each axis's ends in order.
fn rect([a, b]: [[f64; 3]; 2], include_z: bool) -> BoundingRect {
    let order = |a: f64, b: f64| if a > b { (b, a) } else { (a, b) };
    let (minx, maxx) = order(a[0], b[0]);
    let (miny, maxy) = order(a[1], b[1]);
    let (minz, maxz) = order(a[2], b[2]);
    BoundingRect::from_raw_bounds([minx, miny, minz, maxx, maxy, maxz], include_z)
}

#[cfg(test)]
mod test {
    use super::*;

    fn bounds(rect: BoundingRect) -> [f64; 6] {
        [
            rect.minx(),
            rect.miny(),
            rect.minz(),
            rect.maxx(),
            rect.maxy(),
            rect.maxz(),
        ]
    }

    #[test]
    fn test_parse_box2d() {
        let expected = [1.0, 2.0, 0.0, 5.0, 6.0, 0.0];
        for text in [
            "BOX(1 2,5 6)",
            "box(5 6,1 2)",
            "BOX(1 2, 5 6",
            "BOX(5 2,1 6)xyz",
        ] {
            assert_eq!(
                bounds(parse_box2d(text).unwrap())[..2],
                expected[..2],
                "{text}"
            );
        }
        for text in [
            "BOX (1 2,5 6)",
            "BOX(1 2 ,5 6)",
            "BOX(1 2,5)",
            " BOX(1 2,5 6)",
        ] {
            assert!(parse_box2d(text).is_err(), "{text}");
        }
    }

    #[test]
    fn test_parse_box3d() {
        let rect = parse_box3d("BOX3D( 4 5 6 , 1 2 3 )").unwrap();
        assert_eq!(bounds(rect), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let rect = parse_box3d("BOX3D(1 2,4 5)").unwrap();
        assert_eq!(bounds(rect), [1.0, 2.0, 0.0, 4.0, 5.0, 0.0]);
        assert!(parse_box3d("box3d(1 2 3,4 5 6)").is_err());
        assert!(parse_box3d("BOX(1 2,4 5)").is_err());
    }

    #[test]
    fn test_write_box() {
        let rect = parse_box2d("BOX(-0 1e-9,1.00000000000000000001 123456789012345678)").unwrap();
        assert_eq!(write_box2d(&rect), "BOX(0 1e-9,1 1.234567890123457e+17)");
        assert_eq!(
            write_box3d(&rect),
            "BOX3D(0 1e-9 0,1 1.234567890123457e+17 0)"
        );
        let rect = parse_box3d("BOX3D(1.5 2 3,4 5 1e300)").unwrap();
        assert_eq!(write_box3d(&rect), "BOX3D(1.5 2 3,4 5 1e+300)");
        assert_eq!(write_box2d(&rect), "BOX(1.5 2,4 5)");
    }
}
