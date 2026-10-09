//! Reading (E)WKT the way PostGIS does: the input format of the `geometry` type, ST_GeomFromText
//! and untagged text arguments.

use std::fmt;

use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

use crate::util::srid::clamp_srid;

/// Why PostGIS would reject a (E)WKT string. The messages are PostGIS's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WktParseError {
    Invalid,
    MorePointsRequired,
    NonClosedRings,
    MixedDimensions,
    /// Valid in PostGIS, but not representable in GeoArrow.
    Unsupported(String),
}

impl fmt::Display for WktParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WktParseError::Invalid => f.write_str("parse error - invalid geometry"),
            WktParseError::MorePointsRequired => f.write_str("geometry requires more points"),
            WktParseError::NonClosedRings => f.write_str("geometry contains non-closed rings"),
            WktParseError::MixedDimensions => {
                f.write_str("can not mix dimensionality in a geometry")
            }
            WktParseError::Unsupported(what) => write!(f, "{what} is not supported"),
        }
    }
}

/// Parses (E)WKT the way PostGIS does, returning the SRID of a `SRID=n;` prefix, if any.
///
/// Implicit dimensions (`POINT(1 2 3)` is `POINT Z`), `MULTIPOINT(1 2,3 4)`, EMPTY members of
/// multi-geometries and `nan` coordinates are accepted, and PostGIS's validity rules applied:
/// lines need two points, rings four, rings must be closed in 2D, and the coordinates of a
/// geometry, and the members of a collection, must have one dimension.
pub(crate) fn parse_ewkt(text: &str) -> Result<(Option<i32>, Wkt<f64>), WktParseError> {
    let mut parser = Parser {
        text: text.as_bytes(),
        pos: 0,
    };
    let srid = parser.srid_prefix()?;
    let geometry = parser.geometry()?;
    parser.skip_whitespace();
    if parser.pos != parser.text.len() {
        return Err(WktParseError::Invalid);
    }
    Ok((srid, geometry.into_wkt()))
}

/// The SRID of a `SRID=n;` prefix, without parsing the geometry. For literal arguments, whose
/// SRID becomes the output column's CRS when planning.
pub(crate) fn ewkt_srid_prefix(text: &str) -> Option<i32> {
    let mut parser = Parser {
        text: text.as_bytes(),
        pos: 0,
    };
    parser.srid_prefix().ok().flatten()
}

/// A parsed geometry, before conversion to `wkt` types, with its dimension.
enum Parsed {
    Point(Option<Vec<f64>>, Dimension),
    LineString(Vec<Vec<f64>>, Dimension),
    Polygon(Vec<Vec<Vec<f64>>>, Dimension),
    MultiPoint(Vec<Option<Vec<f64>>>, Dimension),
    MultiLineString(Vec<Vec<Vec<f64>>>, Dimension),
    MultiPolygon(Vec<Vec<Vec<Vec<f64>>>>, Dimension),
    GeometryCollection(Vec<Parsed>, Dimension),
}

impl Parsed {
    fn dim(&self) -> Dimension {
        match self {
            Parsed::Point(_, dim)
            | Parsed::LineString(_, dim)
            | Parsed::Polygon(_, dim)
            | Parsed::MultiPoint(_, dim)
            | Parsed::MultiLineString(_, dim)
            | Parsed::MultiPolygon(_, dim)
            | Parsed::GeometryCollection(_, dim) => *dim,
        }
    }

    fn into_wkt(self) -> Wkt<f64> {
        match self {
            Parsed::Point(coord, dim) => Wkt::Point(point(coord, dim)),
            Parsed::LineString(coords, dim) => Wkt::LineString(line_string(coords, dim)),
            Parsed::Polygon(rings, dim) => Wkt::Polygon(polygon(rings, dim)),
            Parsed::MultiPoint(points, dim) => Wkt::MultiPoint(MultiPoint::new(
                points.into_iter().map(|p| point(p, dim)).collect(),
                dim,
            )),
            Parsed::MultiLineString(lines, dim) => Wkt::MultiLineString(MultiLineString::new(
                lines.into_iter().map(|l| line_string(l, dim)).collect(),
                dim,
            )),
            Parsed::MultiPolygon(polygons, dim) => Wkt::MultiPolygon(MultiPolygon::new(
                polygons.into_iter().map(|p| polygon(p, dim)).collect(),
                dim,
            )),
            Parsed::GeometryCollection(members, dim) => Wkt::GeometryCollection(
                GeometryCollection::new(members.into_iter().map(Parsed::into_wkt).collect(), dim),
            ),
        }
    }
}

fn coord(values: Vec<f64>, dim: Dimension) -> Coord<f64> {
    let (z, m) = match dim {
        Dimension::XY => (None, None),
        Dimension::XYZ => (Some(values[2]), None),
        Dimension::XYM => (None, Some(values[2])),
        Dimension::XYZM => (Some(values[2]), Some(values[3])),
    };
    Coord {
        x: values[0],
        y: values[1],
        z,
        m,
    }
}

fn point(values: Option<Vec<f64>>, dim: Dimension) -> Point<f64> {
    Point::new(values.map(|values| coord(values, dim)), dim)
}

fn line_string(coords: Vec<Vec<f64>>, dim: Dimension) -> LineString<f64> {
    LineString::new(coords.into_iter().map(|c| coord(c, dim)).collect(), dim)
}

fn polygon(rings: Vec<Vec<Vec<f64>>>, dim: Dimension) -> Polygon<f64> {
    Polygon::new(
        rings.into_iter().map(|r| line_string(r, dim)).collect(),
        dim,
    )
}

/// Geometry types PostGIS parses that GeoArrow can't represent.
const UNSUPPORTED_TYPES: &[&str] = &[
    "CIRCULARSTRING",
    "COMPOUNDCURVE",
    "CURVEPOLYGON",
    "MULTICURVE",
    "MULTISURFACE",
    "POLYHEDRALSURFACE",
    "TRIANGLE",
    "TIN",
];

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_whitespace(&mut self) {
        while self.text.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    /// Consumes `byte` after optional whitespace, if it's next.
    fn eat(&mut self, byte: u8) -> bool {
        self.skip_whitespace();
        if self.text.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), WktParseError> {
        if self.eat(byte) {
            Ok(())
        } else {
            Err(WktParseError::Invalid)
        }
    }

    /// The next word of ASCII letters, uppercased, without consuming it.
    fn peek_word(&mut self) -> String {
        self.skip_whitespace();
        self.text[self.pos..]
            .iter()
            .take_while(|b| b.is_ascii_alphabetic())
            .map(|b| char::from(b.to_ascii_uppercase()))
            .collect()
    }

    fn take_word(&mut self) -> String {
        let word = self.peek_word();
        self.pos += word.len();
        word
    }

    /// An optional `SRID=n;` prefix.
    fn srid_prefix(&mut self) -> Result<Option<i32>, WktParseError> {
        if self.peek_word() != "SRID" {
            return Ok(None);
        }
        self.take_word();
        self.expect(b'=')?;
        self.skip_whitespace();
        let start = self.pos;
        if self.text.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while self.text.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        let srid: i64 = std::str::from_utf8(&self.text[start..self.pos])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(WktParseError::Invalid)?;
        self.expect(b';')?;
        Ok(Some(clamp_srid(srid)))
    }

    fn geometry(&mut self) -> Result<Parsed, WktParseError> {
        let word = self.take_word();
        // The dimension tag may be glued to the type (POINTZ) or a separate word (POINT Z).
        let (keyword, mut tag) = split_tag(&word);
        if tag.is_none() && matches!(self.peek_word().as_str(), "Z" | "M" | "ZM") {
            tag = split_tag(&self.take_word()).1;
        }
        if UNSUPPORTED_TYPES.contains(&keyword) {
            return Err(WktParseError::Unsupported(format!("{keyword} geometry")));
        }
        let empty = self.peek_word() == "EMPTY";
        if empty {
            self.take_word();
        }
        // An EMPTY geometry without a tag is 2D.
        let mut dim = DimensionState::new(tag.or(empty.then_some(Dimension::XY)));
        let parsed = match keyword {
            "POINT" => {
                let coord = if empty {
                    None
                } else {
                    self.expect(b'(')?;
                    let coord = self.coord(&mut dim)?;
                    self.expect(b')')?;
                    Some(coord)
                };
                Parsed::Point(coord, dim.get())
            }
            "LINESTRING" => {
                let coords = if empty { vec![] } else { self.line(&mut dim)? };
                Parsed::LineString(coords, dim.get())
            }
            "POLYGON" => {
                let rings = if empty { vec![] } else { self.rings(&mut dim)? };
                Parsed::Polygon(rings, dim.get())
            }
            "MULTIPOINT" => {
                let points = if empty {
                    vec![]
                } else {
                    self.list(|parser| {
                        if parser.peek_word() == "EMPTY" {
                            parser.take_word();
                            return Ok(None);
                        }
                        // Members may be written with or without parentheses.
                        let parenthesized = parser.eat(b'(');
                        let coord = parser.coord(&mut dim)?;
                        if parenthesized {
                            parser.expect(b')')?;
                        }
                        Ok(Some(coord))
                    })?
                };
                Parsed::MultiPoint(points, dim.get())
            }
            "MULTILINESTRING" => {
                let lines = if empty {
                    vec![]
                } else {
                    self.list(|parser| {
                        if parser.peek_word() == "EMPTY" {
                            parser.take_word();
                            return Ok(vec![]);
                        }
                        parser.line(&mut dim)
                    })?
                };
                Parsed::MultiLineString(lines, dim.get())
            }
            "MULTIPOLYGON" => {
                let polygons = if empty {
                    vec![]
                } else {
                    self.list(|parser| {
                        if parser.peek_word() == "EMPTY" {
                            parser.take_word();
                            return Ok(vec![]);
                        }
                        parser.rings(&mut dim)
                    })?
                };
                Parsed::MultiPolygon(polygons, dim.get())
            }
            "GEOMETRYCOLLECTION" => {
                let members = if empty {
                    vec![]
                } else {
                    self.list(|parser| {
                        if parser.peek_word() == "GEOMETRYCOLLECTION" {
                            return Err(WktParseError::Unsupported(
                                "a nested GEOMETRYCOLLECTION".to_string(),
                            ));
                        }
                        let member = parser.geometry()?;
                        // Every member must have the collection's dimension.
                        dim.set(member.dim())?;
                        Ok(member)
                    })?
                };
                Parsed::GeometryCollection(members, dim.get())
            }
            _ => return Err(WktParseError::Invalid),
        };
        Ok(parsed)
    }

    /// `( item, item, ... )`.
    fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, WktParseError>,
    ) -> Result<Vec<T>, WktParseError> {
        self.expect(b'(')?;
        let mut items = vec![item(self)?];
        while self.eat(b',') {
            items.push(item(self)?);
        }
        self.expect(b')')?;
        Ok(items)
    }

    /// The coordinates of a line, which needs at least two.
    fn line(&mut self, dim: &mut DimensionState) -> Result<Vec<Vec<f64>>, WktParseError> {
        let coords = self.list(|parser| parser.coord(dim))?;
        if coords.len() < 2 {
            return Err(WktParseError::MorePointsRequired);
        }
        Ok(coords)
    }

    /// The rings of a polygon, which need at least four coordinates and must be closed in 2D.
    fn rings(&mut self, dim: &mut DimensionState) -> Result<Vec<Vec<Vec<f64>>>, WktParseError> {
        let rings = self.list(|parser| parser.list(|parser| parser.coord(dim)))?;
        for ring in &rings {
            if ring.len() < 4 {
                return Err(WktParseError::MorePointsRequired);
            }
            let (first, last) = (&ring[0], &ring[ring.len() - 1]);
            if first[0] != last[0] || first[1] != last[1] {
                return Err(WktParseError::NonClosedRings);
            }
        }
        Ok(rings)
    }

    /// Two to four numbers, which must agree with the geometry's dimension.
    fn coord(&mut self, dim: &mut DimensionState) -> Result<Vec<f64>, WktParseError> {
        let mut values = vec![];
        while let Some(value) = self.number() {
            values.push(value);
        }
        let coord_dim = match values.len() {
            2 => Dimension::XY,
            3 => Dimension::XYZ,
            4 => Dimension::XYZM,
            _ => return Err(WktParseError::Invalid),
        };
        dim.set_from_coord(coord_dim)?;
        Ok(values)
    }

    /// A number as PostGIS's lexer reads it: `-?(digits[.digits]|.digits)([eE][-+]?digits)?`, or
    /// `nan` in any case.
    fn number(&mut self) -> Option<f64> {
        self.skip_whitespace();
        if self.text[self.pos..]
            .get(..3)
            .is_some_and(|word| word.eq_ignore_ascii_case(b"nan"))
        {
            self.pos += 3;
            return Some(f64::NAN);
        }
        let start = self.pos;
        let mut end = start;
        let digits = |end: &mut usize, text: &[u8]| {
            let from = *end;
            while text.get(*end).is_some_and(u8::is_ascii_digit) {
                *end += 1;
            }
            *end > from
        };
        if self.text.get(end) == Some(&b'-') {
            end += 1;
        }
        let integer = digits(&mut end, self.text);
        let mut fraction = false;
        if self.text.get(end) == Some(&b'.') {
            end += 1;
            fraction = digits(&mut end, self.text);
        }
        if !integer && !fraction {
            return None;
        }
        if matches!(self.text.get(end), Some(b'e' | b'E')) {
            let mut exponent_end = end + 1;
            if matches!(self.text.get(exponent_end), Some(b'-' | b'+')) {
                exponent_end += 1;
            }
            if digits(&mut exponent_end, self.text) {
                end = exponent_end;
            }
        }
        let value = std::str::from_utf8(&self.text[start..end])
            .ok()?
            .parse()
            .ok()?;
        self.pos = end;
        Some(value)
    }
}

/// Geometry types, as their WKT keywords. None of them ends in a dimension tag letter.
const TYPES: &[&str] = &[
    "POINT",
    "LINESTRING",
    "POLYGON",
    "MULTIPOINT",
    "MULTILINESTRING",
    "MULTIPOLYGON",
    "GEOMETRYCOLLECTION",
];

/// Splits a dimension tag off a word: `POINTZM` is `("POINT", Some(XYZM))`, and a lone `ZM` is
/// `("", Some(XYZM))`.
fn split_tag(word: &str) -> (&str, Option<Dimension>) {
    for (suffix, dim) in [
        ("ZM", Dimension::XYZM),
        ("Z", Dimension::XYZ),
        ("M", Dimension::XYM),
    ] {
        if let Some(keyword) = word.strip_suffix(suffix)
            && (keyword.is_empty()
                || TYPES.contains(&keyword)
                || UNSUPPORTED_TYPES.contains(&keyword))
        {
            return (keyword, Some(dim));
        }
    }
    (word, None)
}

/// The dimension of the geometry being parsed: fixed by a tag, or by its first coordinate.
struct DimensionState {
    dim: Option<Dimension>,
    tagged: bool,
}

impl DimensionState {
    fn new(tag: Option<Dimension>) -> Self {
        Self {
            dim: tag,
            tagged: tag.is_some(),
        }
    }

    fn get(&self) -> Dimension {
        self.dim.unwrap_or(Dimension::XY)
    }

    /// Fixes the dimension, or checks it against the one already fixed.
    fn set(&mut self, dim: Dimension) -> Result<(), WktParseError> {
        match self.dim {
            None => {
                self.dim = Some(dim);
                Ok(())
            }
            Some(existing) if existing == dim => Ok(()),
            Some(_) => Err(WktParseError::MixedDimensions),
        }
    }

    /// Like [`Self::set`] for a coordinate's dimension. Under an `M` tag, three values are XYM.
    fn set_from_coord(&mut self, coord_dim: Dimension) -> Result<(), WktParseError> {
        if self.tagged && self.dim == Some(Dimension::XYM) && coord_dim == Dimension::XYZ {
            return Ok(());
        }
        self.set(coord_dim)
    }
}
