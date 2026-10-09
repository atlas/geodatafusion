//! Google's Encoded Polyline Algorithm Format, as PostGIS reads and writes it.

/// PostGIS's default number of decimal digits.
pub(crate) const DEFAULT_PRECISION: i32 = 5;

/// The scale factor for `precision` decimal digits. A negative precision means the default, as
/// in PostGIS.
fn factor(precision: i32) -> f64 {
    let precision = if precision < 0 {
        DEFAULT_PRECISION
    } else {
        precision
    };
    10f64.powi(precision)
}

/// Encodes `(x, y)` positions, latitude (y) first in each pair. Each value is rounded half away
/// from zero to `precision` decimals and written as the difference to the previous one.
///
/// The integer arithmetic is PostGIS's, fitted to its output: a difference that doesn't fit 32
/// bits becomes `i32::MIN` (as x86 converts it), and the zigzag shift wraps in 32 bits. So, as
/// there, precisions above 7 give garbage, and so does 7 for longitudes beyond about 107 degrees.
pub(crate) fn encode(positions: impl IntoIterator<Item = (f64, f64)>, precision: i32) -> String {
    let factor = factor(precision);
    let mut out = String::new();
    let (mut previous_lat, mut previous_lon) = (0.0, 0.0);
    for (x, y) in positions {
        let lat = (y * factor).round();
        let lon = (x * factor).round();
        encode_value(&mut out, to_i32(lat - previous_lat));
        encode_value(&mut out, to_i32(lon - previous_lon));
        (previous_lat, previous_lon) = (lat, lon);
    }
    out
}

/// C's conversion to `int` on x86: out of range, and NaN, is `i32::MIN`.
fn to_i32(value: f64) -> i32 {
    if value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) {
        value as i32
    } else {
        i32::MIN
    }
}

fn encode_value(out: &mut String, value: i32) {
    // Zigzag: the sign moves to the lowest bit.
    let shifted = value.wrapping_shl(1);
    let mut value = if shifted < 0 { !shifted } else { shifted } as u32;
    while value >= 0x20 {
        out.push(char::from((0x20 | (value & 0x1f) as u8) + 63));
        value >>= 5;
    }
    out.push(char::from(value as u8 + 63));
}

/// Decodes positions as `(x, y)`, reading latitude first in each pair.
///
/// Like PostGIS, a value cut off by the end of the text reads the end as a NUL character: one
/// final chunk of 1. Characters below `?` end a value the same way.
pub(crate) fn decode(text: &str, precision: i32) -> Vec<(f64, f64)> {
    let factor = factor(precision);
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut positions = Vec::new();
    let (mut lat, mut lon) = (0i64, 0i64);
    while index < bytes.len() {
        lat += decode_value(bytes, &mut index);
        lon += decode_value(bytes, &mut index);
        positions.push((lon as f64 / factor, lat as f64 / factor));
    }
    positions
}

/// The value starting at `index`, moving `index` past it.
fn decode_value(bytes: &[u8], index: &mut usize) -> i64 {
    let mut result: i64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = bytes.get(*index).copied().unwrap_or(0);
        *index += 1;
        let chunk = i64::from(byte) - 63;
        result |= (chunk & 0x1f).checked_shl(shift).unwrap_or(0);
        shift += 5;
        if chunk < 0x20 {
            break;
        }
    }
    if result & 1 == 1 {
        !(result >> 1)
    } else {
        result >> 1
    }
}
