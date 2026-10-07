//! PostGIS's buffer style parameters, shared by ST_Buffer and ST_OffsetCurve.

use datafusion::common::exec_datafusion_err;
use geos::{BufferParams, CapStyle, JoinStyle};

use crate::error::GeoDataFusionResult;

/// Which side of a line ST_Buffer buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Both,
    Left,
    Right,
}

/// Which style keys a function accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StyleKeys {
    /// ST_Buffer: every key.
    Buffer,
    /// ST_OffsetCurve: `join`, `mitre_limit`/`miter_limit` and `quad_segs`.
    OffsetCurve,
}

/// The style of a PostGIS buffer, with PostGIS's defaults.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BufferStyle {
    pub(crate) quad_segs: i32,
    pub(crate) end_cap: CapStyle,
    pub(crate) join: JoinStyle,
    pub(crate) mitre_limit: f64,
    pub(crate) side: Side,
}

impl Default for BufferStyle {
    fn default() -> Self {
        Self {
            quad_segs: 8,
            end_cap: CapStyle::Round,
            join: JoinStyle::Round,
            mitre_limit: 5.0,
            side: Side::Both,
        }
    }
}

impl BufferStyle {
    /// Parses a PostGIS style string: space-separated `key=value` pairs. Keys and values are
    /// case-sensitive; an unknown key or value, or a missing value, is an error. Numbers are read
    /// like C's `atoi` and `atof`, as PostGIS does (`quad_segs=2.7` is 2, `abc` is 0).
    pub(crate) fn parse(name: &str, params: &str, keys: StyleKeys) -> GeoDataFusionResult<Self> {
        let mut style = Self::default();
        for pair in params.split(' ').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if key.is_empty()
                || (keys == StyleKeys::OffsetCurve && matches!(key, "endcap" | "side"))
            {
                return Err(invalid_parameter(name, key, keys).into());
            }
            if value.is_empty()
                && matches!(
                    key,
                    "endcap" | "join" | "mitre_limit" | "miter_limit" | "quad_segs" | "side"
                )
            {
                return Err(exec_datafusion_err!(
                    "{name}: Missing value for buffer parameter {pair}"
                )
                .into());
            }
            match key {
                "endcap" => {
                    style.end_cap = match value {
                        "round" => CapStyle::Round,
                        "flat" | "butt" => CapStyle::Flat,
                        "square" => CapStyle::Square,
                        _ => {
                            return Err(exec_datafusion_err!(
                                "{name}: Invalid buffer end cap style: {value} (accept: 'round', \
                                 'flat', 'butt' or 'square')"
                            )
                            .into());
                        }
                    }
                }
                "join" => {
                    style.join = match value {
                        "round" => JoinStyle::Round,
                        "mitre" | "miter" => JoinStyle::Mitre,
                        "bevel" => JoinStyle::Bevel,
                        _ => {
                            return Err(exec_datafusion_err!(
                                "{name}: Invalid buffer join style: {value} (accept: 'round', \
                                 'mitre', 'miter' or 'bevel')"
                            )
                            .into());
                        }
                    }
                }
                "mitre_limit" | "miter_limit" => style.mitre_limit = atof(value),
                "quad_segs" => style.quad_segs = atoi(value),
                "side" => {
                    style.side = match value {
                        "both" => Side::Both,
                        "left" => Side::Left,
                        "right" => Side::Right,
                        _ => {
                            return Err(exec_datafusion_err!(
                                "{name}: Invalid side parameter: {value} (accept: 'right', \
                                 'left', 'both')"
                            )
                            .into());
                        }
                    }
                }
                _ => return Err(invalid_parameter(name, key, keys).into()),
            }
        }
        Ok(style)
    }

    /// The GEOS buffer parameters. A one-sided buffer is a GEOS single-sided buffer.
    pub(crate) fn buffer_params(&self) -> GeoDataFusionResult<BufferParams> {
        Ok(BufferParams::builder()
            .quadrant_segments(self.quad_segs)
            .end_cap_style(self.end_cap)
            .join_style(self.join)
            .mitre_limit(self.mitre_limit)
            .single_sided(self.side != Side::Both)
            .build()?)
    }

    /// The GEOS buffer width for a radius: a buffer on the right side has a negative width.
    pub(crate) fn width(&self, radius: f64) -> f64 {
        if self.side == Side::Right {
            -radius
        } else {
            radius
        }
    }
}

fn invalid_parameter(name: &str, key: &str, keys: StyleKeys) -> datafusion::error::DataFusionError {
    let accepted = match keys {
        StyleKeys::Buffer => {
            "'endcap', 'join', 'mitre_limit', 'miter_limit', 'quad_segs' and 'side'"
        }
        StyleKeys::OffsetCurve => "'join', 'mitre_limit', 'miter_limit' and 'quad_segs'",
    };
    exec_datafusion_err!("{name}: Invalid buffer parameter: {key} (accept: {accepted})")
}

/// The leading integer of `text`, as C's `atoi` reads it: optional whitespace and sign, then
/// digits; 0 without any.
fn atoi(text: &str) -> i32 {
    let text = text.trim_start();
    let (sign, digits) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    sign * digits[..end].parse::<i32>().unwrap_or(0)
}

/// The leading number of `text`, as C's `atof` reads it; 0 without one.
fn atof(text: &str) -> f64 {
    let text = text.trim_start();
    (1..=text.len())
        .rev()
        .filter(|&end| text.is_char_boundary(end))
        .find_map(|end| text[..end].parse::<f64>().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_atoi_and_atof_read_leading_numbers() {
        assert_eq!(atoi("2.7"), 2);
        assert_eq!(atoi("abc"), 0);
        assert_eq!(atoi("-3"), -3);
        assert_eq!(atoi("1,endcap=flat"), 1);
        assert_eq!(atof("1.5x"), 1.5);
        assert_eq!(atof("abc"), 0.0);
    }
}
