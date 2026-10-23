//! Reading numbers out of text the way PostGIS's C parsers do, with `strtod`.

/// The number at the start of `text`, after any whitespace, and the text after it: the longest
/// prefix that reads as a number, as `strtod` reads it. That includes `inf`, `nan` and
/// hexadecimal integers (`0x10`).
pub(crate) fn scan_number(text: &str) -> Option<(f64, &str)> {
    let text = text.trim_start();
    if let Some(hex) = scan_hex(text) {
        return Some(hex);
    }
    (1..=text.len())
        .rev()
        .filter(|&end| text.is_char_boundary(end))
        .find_map(|end| Some((text[..end].parse().ok()?, &text[end..])))
}

/// A hexadecimal integer with an optional sign, such as `-0x1F`.
fn scan_hex(text: &str) -> Option<(f64, &str)> {
    let (negative, unsigned) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let digits = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))?;
    let end = digits
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let value = digits[..end].chars().fold(0.0, |value, digit| {
        value * 16.0 + f64::from(digit.to_digit(16).unwrap_or(0))
    });
    Some((if negative { -value } else { value }, &digits[end..]))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_scan_number() {
        assert_eq!(scan_number(" 1e1 2"), Some((10.0, " 2")));
        assert_eq!(scan_number("0x10,"), Some((16.0, ",")));
        assert_eq!(scan_number("-2.5)"), Some((-2.5, ")")));
        assert_eq!(scan_number("inf "), Some((f64::INFINITY, " ")));
        assert!(scan_number("nan").is_some_and(|(value, _)| value.is_nan()));
        assert_eq!(scan_number(",1"), None);
    }
}
