//! Number formatting for text output, matching PostGIS.

/// PostGIS's default `maxdecimaldigits` for WKT, KML, GML and SVG.
pub(crate) const DEFAULT_MAX_DECIMAL_DIGITS: i32 = 15;

/// Absolute values in `(FIXED_MIN, FIXED_MAX)` are written in fixed notation, others in
/// exponential notation.
const FIXED_MIN: f64 = 1e-8;
const FIXED_MAX: f64 = 1e15;

/// Writes `value` the way PostGIS writes coordinates.
///
/// Takes the shortest decimal digits that round-trip, rounds them half-to-even to at most
/// `max_decimal_digits` decimals (negative counts as 0), and drops trailing zeros. In fixed
/// notation the decimals are after the decimal point; in exponential notation they are the
/// mantissa's, and a carry out of the mantissa keeps the exponent (`9.99e-9` with 0 decimals is
/// `10e-9`). The notation is chosen before rounding. Zero, and anything that rounds to zero, is
/// `0` without a sign.
///
/// The rules were derived from PostGIS's output and are checked against it by the test data in
/// `testdata/number_format.txt`.
pub(crate) fn write_number(out: &mut String, value: f64, max_decimal_digits: i32) {
    if value.is_nan() {
        out.push_str("NaN");
        return;
    }
    if value.is_infinite() {
        out.push_str(if value > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }
    if value == 0.0 {
        out.push('0');
        return;
    }
    let decimals = i64::from(max_decimal_digits.max(0));
    let fixed = value.abs() > FIXED_MIN && value.abs() < FIXED_MAX;
    let (mut digits, mut exponent) = shortest_digits(value.abs());

    // The number of leading digits to keep.
    let keep = if fixed {
        exponent + decimals + 1
    } else {
        decimals + 1
    };
    let mut carried = false;
    if keep < digits.len() as i64 {
        if keep < 0 {
            digits.clear();
        } else {
            let keep = keep as usize;
            let first_dropped = digits[keep];
            let rest_nonzero = digits[keep + 1..].iter().any(|d| *d != 0);
            let last_kept_odd = keep > 0 && digits[keep - 1] % 2 == 1;
            digits.truncate(keep);
            if first_dropped > 5 || (first_dropped == 5 && (rest_nonzero || last_kept_odd)) {
                carried = increment(&mut digits);
                if carried && fixed {
                    exponent += 1;
                }
            }
        }
    }
    // A carry in exponential notation widens the mantissa to two digits instead.
    let mantissa_digits = if carried && !fixed { 2 } else { 1 };
    while digits.len() > mantissa_digits && digits.last() == Some(&0) {
        digits.pop();
    }
    if digits.iter().all(|d| *d == 0) {
        out.push('0');
        return;
    }

    if value < 0.0 {
        out.push('-');
    }
    if fixed {
        write_fixed_digits(out, &digits, exponent);
    } else {
        push_digits(out, &digits[..mantissa_digits]);
        if digits.len() > mantissa_digits {
            out.push('.');
            push_digits(out, &digits[mantissa_digits..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&exponent.abs().to_string());
    }
}

/// The shortest round-trip decimal digits of a positive, finite `value`, and the decimal
/// exponent of the first digit.
///
/// When several decimals of the shortest length round-trip, this is the one closest to `value`,
/// with exact ties to even, as PostGIS picks it.
fn shortest_digits(value: f64) -> (Vec<u8>, i64) {
    // `{:e}` writes the shortest digits that round-trip, but doesn't always pick the closest of
    // several candidates, so it only gives the length here.
    let length = format!("{value:e}")
        .split('e')
        .next()
        .map_or(1, |mantissa| {
            mantissa.bytes().filter(u8::is_ascii_digit).count()
        });
    // Exact formatting rounds correctly, ties to even, so it gives the closest decimal of that
    // length, which round-trips too.
    let scientific = format!("{value:.*e}", length.saturating_sub(1));
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` always writes an exponent");
    let digits = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    let exponent = exponent.parse().expect("`{:e}` writes an integer exponent");
    (digits, exponent)
}

/// Adds one unit in the last place. Returns whether it carried out of the first digit, in which
/// case a `1` is prepended.
fn increment(digits: &mut Vec<u8>) -> bool {
    for digit in digits.iter_mut().rev() {
        if *digit == 9 {
            *digit = 0;
        } else {
            *digit += 1;
            return false;
        }
    }
    digits.insert(0, 1);
    true
}

fn write_fixed_digits(out: &mut String, digits: &[u8], exponent: i64) {
    if exponent >= 0 {
        let integer_digits = (exponent + 1) as usize;
        for i in 0..integer_digits {
            out.push(char::from(b'0' + digits.get(i).copied().unwrap_or(0)));
        }
        if digits.len() > integer_digits {
            out.push('.');
            push_digits(out, &digits[integer_digits..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        push_digits(out, digits);
    }
}

fn push_digits(out: &mut String, digits: &[u8]) {
    out.extend(digits.iter().map(|d| char::from(b'0' + d)));
}

#[cfg(test)]
mod test {
    use super::*;

    fn format(value: f64, max_decimal_digits: i32) -> String {
        let mut out = String::new();
        write_number(&mut out, value, max_decimal_digits);
        out
    }

    #[test]
    fn test_write_number_matches_postgis() {
        let cases = include_str!("testdata/number_format.txt");
        let mut mismatches = vec![];
        for line in cases.lines().filter(|line| !line.starts_with('#')) {
            let mut fields = line.split('|');
            let (Some(value), Some(digits), Some(expected)) =
                (fields.next(), fields.next(), fields.next())
            else {
                panic!("malformed test case {line:?}");
            };
            let value: f64 = value.parse().unwrap();
            let digits: i32 = digits.parse().unwrap();
            let actual = format(value, digits);
            if actual != expected {
                mismatches.push(format!("{value:e} with {digits}: {actual} != {expected}"));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }

    #[test]
    fn test_write_number_special_values() {
        assert_eq!(format(f64::NAN, 15), "NaN");
        assert_eq!(format(f64::INFINITY, 15), "Infinity");
        assert_eq!(format(f64::NEG_INFINITY, 15), "-Infinity");
        assert_eq!(format(-0.0, 15), "0");
    }
}
