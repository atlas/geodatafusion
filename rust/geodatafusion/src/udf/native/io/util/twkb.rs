//! Tiny Well-Known Binary (TWKB), as specified at <https://github.com/TWKB/Specification>, with
//! PostGIS's choices where the specification leaves them open.
//!
//! A geometry starts with a header byte (the type in the low four bits, the zigzagged XY
//! precision in the high four) and a metadata byte (flags for a bounding box, a size, an ID list,
//! extended dimensions and emptiness). Then come the extended dimensions byte, the size, the
//! bounding box and the body, each if flagged. Coordinates are integers, scaled by ten to the
//! precision, written as zigzagged varints of the difference to the previous point.

use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
};
use wkt::Wkt;
use wkt::types::{
    Coord, Dimension, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};

const BBOX: u8 = 0x01;
const SIZE: u8 = 0x02;
const ID_LIST: u8 = 0x04;
const EXTENDED_DIMENSIONS: u8 = 0x08;
const EMPTY: u8 = 0x10;

const POINT: u8 = 1;
const LINE_STRING: u8 = 2;
const POLYGON: u8 = 3;
const MULTI_POINT: u8 = 4;
const MULTI_LINE_STRING: u8 = 5;
const MULTI_POLYGON: u8 = 6;
const GEOMETRY_COLLECTION: u8 = 7;

/// How to write TWKB: decimal digits per ordinate, and whether to write sizes and bounding boxes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Options {
    pub(crate) precision_xy: i32,
    pub(crate) precision_z: i32,
    pub(crate) precision_m: i32,
    pub(crate) sizes: bool,
    pub(crate) boxes: bool,
}

/// Writes a geometry as TWKB, or returns PostGIS's message for a precision out of range.
///
/// Values round half away from zero. As in PostGIS, a line or ring leaves out a point equal to
/// the previous one once rounded, as long as enough points are left: two for a line, four for a
/// ring. A multipoint's empty points are left out; the empty parts of other geometries stay.
pub(crate) fn write(
    geom: &impl GeometryTrait<T = f64>,
    options: &Options,
) -> Result<Vec<u8>, String> {
    if !(-7..=7).contains(&options.precision_xy) {
        return Err("X/Z precision cannot be greater than 7 or less than -7".to_string());
    }
    let (has_z, has_m) = z_m(geom.dim());
    if has_z && !(0..=7).contains(&options.precision_z) {
        return Err("Z precision cannot be negative or greater than 7".to_string());
    }
    if has_m && !(0..=7).contains(&options.precision_m) {
        return Err("M precision cannot be negative or greater than 7".to_string());
    }
    let mut out = Vec::new();
    write_geometry(&mut out, geom, options)?;
    Ok(out)
}

fn z_m(dim: Dimensions) -> (bool, bool) {
    match dim {
        Dimensions::Xy | Dimensions::Unknown(2) => (false, false),
        Dimensions::Xyz | Dimensions::Unknown(3) => (true, false),
        Dimensions::Xym => (false, true),
        Dimensions::Xyzm | Dimensions::Unknown(_) => (true, true),
    }
}

/// `(min, max)` per ordinate.
type Bbox = [(i64, i64); 4];

/// Writes a geometry, header and all, and returns the bounding box of its points.
fn write_geometry(
    out: &mut Vec<u8>,
    geom: &impl GeometryTrait<T = f64>,
    options: &Options,
) -> Result<Option<Bbox>, String> {
    let kind = match geom.as_type() {
        GeometryType::Point(_) => POINT,
        GeometryType::LineString(_) => LINE_STRING,
        GeometryType::Polygon(_) => POLYGON,
        GeometryType::MultiPoint(_) => MULTI_POINT,
        GeometryType::MultiLineString(_) => MULTI_LINE_STRING,
        GeometryType::MultiPolygon(_) => MULTI_POLYGON,
        GeometryType::GeometryCollection(_) => GEOMETRY_COLLECTION,
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {
            return Err("Unsupported geometry type".to_string());
        }
    };
    let mut writer = Writer::new(geom.dim(), options);
    out.push(kind | ((zigzag(i64::from(options.precision_xy)) as u8) << 4));
    let empty = is_empty(geom);
    let extended = writer.size > 2;
    let mut metadata = 0;
    if extended {
        metadata |= EXTENDED_DIMENSIONS;
    }
    if options.sizes {
        metadata |= SIZE;
    }
    if empty {
        metadata |= EMPTY;
    } else if options.boxes {
        metadata |= BBOX;
    }
    out.push(metadata);
    if extended {
        // The precisions are written even for an ordinate the geometry doesn't have.
        let (has_z, has_m) = z_m(geom.dim());
        out.push(
            u8::from(has_z)
                | (u8::from(has_m) << 1)
                | (((options.precision_z & 7) as u8) << 2)
                | (((options.precision_m & 7) as u8) << 5),
        );
    }
    if empty {
        if options.sizes {
            out.push(0);
        }
        return Ok(None);
    }
    let mut body = Vec::new();
    match geom.as_type() {
        GeometryType::Point(point) => writer.point(&mut body, point),
        GeometryType::LineString(line) => writer.line(&mut body, line, 2),
        GeometryType::Polygon(polygon) => writer.polygon(&mut body, polygon),
        GeometryType::MultiPoint(points) => {
            let points: Vec<_> = points.points().filter(|p| p.coord().is_some()).collect();
            write_varint(&mut body, points.len() as u64);
            for point in &points {
                writer.point(&mut body, point);
            }
        }
        GeometryType::MultiLineString(lines) => {
            write_varint(&mut body, lines.num_line_strings() as u64);
            for line in lines.line_strings() {
                writer.line(&mut body, &line, 2);
            }
        }
        GeometryType::MultiPolygon(polygons) => {
            write_varint(&mut body, polygons.num_polygons() as u64);
            for polygon in polygons.polygons() {
                writer.polygon(&mut body, &polygon);
            }
        }
        // Each member is a geometry of its own, with its own header and starting point.
        GeometryType::GeometryCollection(collection) => {
            write_varint(&mut body, collection.num_geometries() as u64);
            for member in collection.geometries() {
                let bbox = write_geometry(&mut body, &member, options)?;
                writer.bbox = merge(writer.bbox, bbox);
            }
        }
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => {}
    }
    let mut bbox = Vec::new();
    if let Some(extent) = writer.bbox.filter(|_| options.boxes) {
        for &(min, max) in &extent[..writer.size] {
            write_varint(&mut bbox, zigzag(min));
            write_varint(&mut bbox, zigzag(max.wrapping_sub(min)));
        }
    }
    if options.sizes {
        write_varint(out, (bbox.len() + body.len()) as u64);
    }
    out.extend(bbox);
    out.extend(body);
    Ok(writer.bbox)
}

/// The state of writing one geometry's points.
struct Writer {
    /// Ordinates per point, and the scale factor of each.
    size: usize,
    factors: [f64; 4],
    previous: [i64; 4],
    bbox: Option<Bbox>,
}

impl Writer {
    fn new(dim: Dimensions, options: &Options) -> Self {
        let (has_z, has_m) = z_m(dim);
        let xy = scale(options.precision_xy);
        let mut factors = [xy, xy, 0.0, 0.0];
        let mut size = 2;
        if has_z {
            factors[size] = scale(options.precision_z);
            size += 1;
        }
        if has_m {
            factors[size] = scale(options.precision_m);
            size += 1;
        }
        Writer {
            size,
            factors,
            previous: [0; 4],
            bbox: None,
        }
    }

    /// The coordinate scaled and rounded, `i64::MIN` if that doesn't fit, as x86 converts it.
    fn quantize(&self, coord: &impl CoordTrait<T = f64>) -> [i64; 4] {
        let mut values = [0; 4];
        for (index, value) in values.iter_mut().enumerate().take(self.size) {
            let scaled = (coord.nth_or_panic(index) * self.factors[index]).round();
            *value = if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&scaled) {
                scaled as i64
            } else {
                i64::MIN
            };
        }
        values
    }

    fn write_coord(&mut self, out: &mut Vec<u8>, values: [i64; 4]) {
        let bbox = self.bbox.get_or_insert([(i64::MAX, i64::MIN); 4]);
        for index in 0..self.size {
            write_varint(
                out,
                zigzag(values[index].wrapping_sub(self.previous[index])),
            );
            bbox[index] = (
                bbox[index].0.min(values[index]),
                bbox[index].1.max(values[index]),
            );
        }
        self.previous = values;
    }

    fn point(&mut self, out: &mut Vec<u8>, point: &impl PointTrait<T = f64>) {
        if let Some(coord) = point.coord() {
            let values = self.quantize(&coord);
            self.write_coord(out, values);
        }
    }

    /// Writes the number of points and the points, keeping at least `minimum` of them.
    fn line(&mut self, out: &mut Vec<u8>, line: &impl LineStringTrait<T = f64>, minimum: usize) {
        let points = line.coords().map(|coord| self.quantize(&coord)).collect();
        let points = drop_repeats(points, minimum);
        write_varint(out, points.len() as u64);
        for values in points {
            self.write_coord(out, values);
        }
    }

    fn polygon(&mut self, out: &mut Vec<u8>, polygon: &impl PolygonTrait<T = f64>) {
        let rings: Vec<_> = polygon
            .exterior()
            .into_iter()
            .chain(polygon.interiors())
            .collect();
        write_varint(out, rings.len() as u64);
        for ring in &rings {
            self.line(out, ring, 4);
        }
    }
}

/// The factor to multiply values by before rounding, in single precision as PostGIS's output
/// shows: 0.01 is a little less than a hundredth, so 150 at precision -2 rounds to 100, while 0.1
/// is a little more than a tenth, so 15 at precision -1 rounds to 20. Positive powers of ten up
/// to 7 are exact.
fn scale(precision: i32) -> f64 {
    f64::from(10f64.powi(precision) as f32)
}

/// The points, leaving out those equal to the previous one while that leaves at least `minimum`.
fn drop_repeats(points: Vec<[i64; 4]>, minimum: usize) -> Vec<[i64; 4]> {
    let count = points.len();
    let mut kept: Vec<[i64; 4]> = Vec::with_capacity(count);
    let mut dropped = 0;
    for values in points {
        if kept.last() == Some(&values) && count - dropped > minimum {
            dropped += 1;
            continue;
        }
        kept.push(values);
    }
    kept
}

fn merge(a: Option<Bbox>, b: Option<Bbox>) -> Option<Bbox> {
    match (a, b) {
        (Some(mut a), Some(b)) => {
            for (a, b) in a.iter_mut().zip(b) {
                *a = (a.0.min(b.0), a.1.max(b.1));
            }
            Some(a)
        }
        (a, b) => a.or(b),
    }
}

/// Whether a geometry has no points at all, which TWKB flags rather than writing a body.
fn is_empty(geom: &impl GeometryTrait<T = f64>) -> bool {
    match geom.as_type() {
        GeometryType::Point(point) => point.coord().is_none(),
        GeometryType::LineString(line) => line.num_coords() == 0,
        GeometryType::Polygon(polygon) => polygon.exterior().is_none(),
        GeometryType::MultiPoint(points) => points.points().all(|p| p.coord().is_none()),
        GeometryType::MultiLineString(lines) => lines.line_strings().all(|l| l.num_coords() == 0),
        GeometryType::MultiPolygon(polygons) => polygons.polygons().all(|p| p.exterior().is_none()),
        GeometryType::GeometryCollection(collection) => {
            collection.geometries().all(|member| is_empty(&member))
        }
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => false,
    }
}

fn zigzag(value: i64) -> u64 {
    (value.wrapping_shl(1) ^ (value >> 63)) as u64
}

fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Reads a geometry from TWKB, or returns PostGIS's message for bad input.
///
/// As in PostGIS, sizes, bounding boxes, ID lists and bytes after the geometry are skipped
/// unchecked, a ring not closed in X and Y is closed, and a line needs two points and a ring four.
pub(crate) fn read(buffer: &[u8]) -> Result<Wkt<f64>, String> {
    Reader {
        buffer,
        position: 0,
    }
    .geometry()
}

struct Reader<'a> {
    buffer: &'a [u8],
    position: usize,
}

/// The ordinates of a geometry being read.
struct Layout {
    dim: Dimension,
    has_z: bool,
    has_m: bool,
    size: usize,
    factors: [f64; 4],
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let byte = self.buffer.get(self.position).copied().ok_or_else(|| {
            "twkb_parse_state_advance: TWKB structure does not match expected size!".to_string()
        })?;
        self.position += 1;
        Ok(byte)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.buffer.get(self.position).copied().ok_or_else(|| {
                "varint_u64_decode: varint extends past end of buffer".to_string()
            })?;
            self.position += 1;
            value |= u64::from(byte & 0x7f).checked_shl(shift).unwrap_or(0);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn count(&mut self) -> Result<usize, String> {
        // A count can't be larger than the bytes left: each item takes at least one.
        let count = self.varint()?;
        let left = self.buffer.len() - self.position;
        Ok(usize::try_from(count).map_or(left + 1, |count| count.min(left + 1)))
    }

    fn geometry(&mut self) -> Result<Wkt<f64>, String> {
        let header = self.byte()?;
        let kind = header & 0x0f;
        let precision_xy = unzigzag(u64::from(header >> 4)) as i32;
        let metadata = self.byte()?;
        let (mut has_z, mut has_m, mut precision_z, mut precision_m) = (false, false, 0, 0);
        if metadata & EXTENDED_DIMENSIONS != 0 {
            let extended = self.byte()?;
            has_z = extended & 1 != 0;
            has_m = extended & 2 != 0;
            precision_z = i32::from((extended >> 2) & 7);
            precision_m = i32::from((extended >> 5) & 7);
        }
        if !(POINT..=GEOMETRY_COLLECTION).contains(&kind) {
            return Err("Unknown WKB type".to_string());
        }
        let xy = 10f64.powi(precision_xy);
        let mut factors = [xy, xy, 0.0, 0.0];
        let mut size = 2;
        if has_z {
            factors[size] = 10f64.powi(precision_z);
            size += 1;
        }
        if has_m {
            factors[size] = 10f64.powi(precision_m);
            size += 1;
        }
        let layout = Layout {
            dim: match (has_z, has_m) {
                (false, false) => Dimension::XY,
                (true, false) => Dimension::XYZ,
                (false, true) => Dimension::XYM,
                (true, true) => Dimension::XYZM,
            },
            has_z,
            has_m,
            size,
            factors,
        };
        if metadata & SIZE != 0 {
            self.varint()?;
        }
        // As in PostGIS, a flagged bounding box is read even for an empty geometry.
        if metadata & BBOX != 0 {
            for _ in 0..2 * size {
                self.varint()?;
            }
        }
        let dim = layout.dim;
        if metadata & EMPTY != 0 {
            return Ok(empty(kind, dim));
        }
        let mut previous = [0i64; 4];
        let mut points = Points {
            reader: self,
            layout: &layout,
            previous: &mut previous,
        };
        Ok(match kind {
            POINT => Wkt::Point(Point::new(Some(points.coord()?), dim)),
            LINE_STRING => Wkt::LineString(points.line("LineString")?),
            POLYGON => Wkt::Polygon(points.polygon()?),
            MULTI_POINT => {
                let count = points.reader.count()?;
                points.skip_ids(metadata, count)?;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(Point::new(Some(points.coord()?), dim));
                }
                Wkt::MultiPoint(MultiPoint::new(members, dim))
            }
            MULTI_LINE_STRING => {
                let count = points.reader.count()?;
                points.skip_ids(metadata, count)?;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(points.line("MultiLineString")?);
                }
                Wkt::MultiLineString(MultiLineString::new(members, dim))
            }
            MULTI_POLYGON => {
                let count = points.reader.count()?;
                points.skip_ids(metadata, count)?;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(points.polygon()?);
                }
                Wkt::MultiPolygon(MultiPolygon::new(members, dim))
            }
            _ => {
                let count = points.reader.count()?;
                points.skip_ids(metadata, count)?;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(points.reader.geometry()?);
                }
                Wkt::GeometryCollection(GeometryCollection::new(members, dim))
            }
        })
    }
}

fn empty(kind: u8, dim: Dimension) -> Wkt<f64> {
    match kind {
        POINT => Wkt::Point(Point::new(None, dim)),
        LINE_STRING => Wkt::LineString(LineString::new(vec![], dim)),
        POLYGON => Wkt::Polygon(Polygon::new(vec![], dim)),
        MULTI_POINT => Wkt::MultiPoint(MultiPoint::new(vec![], dim)),
        MULTI_LINE_STRING => Wkt::MultiLineString(MultiLineString::new(vec![], dim)),
        MULTI_POLYGON => Wkt::MultiPolygon(MultiPolygon::new(vec![], dim)),
        _ => Wkt::GeometryCollection(GeometryCollection::new(vec![], dim)),
    }
}

/// Reads the points of one geometry, each relative to the one before.
struct Points<'r, 'a> {
    reader: &'r mut Reader<'a>,
    layout: &'r Layout,
    previous: &'r mut [i64; 4],
}

impl Points<'_, '_> {
    fn skip_ids(&mut self, metadata: u8, count: usize) -> Result<(), String> {
        if metadata & ID_LIST != 0 {
            for _ in 0..count {
                self.reader.varint()?;
            }
        }
        Ok(())
    }

    fn coord(&mut self) -> Result<Coord<f64>, String> {
        let mut values = [0.0; 4];
        for (index, value) in values.iter_mut().enumerate().take(self.layout.size) {
            let delta = unzigzag(self.reader.varint()?);
            self.previous[index] = self.previous[index].wrapping_add(delta);
            *value = self.previous[index] as f64 / self.layout.factors[index];
        }
        let mut ordinates = values[2..self.layout.size].iter().copied();
        Ok(Coord {
            x: values[0],
            y: values[1],
            z: self.layout.has_z.then(|| ordinates.next()).flatten(),
            m: self.layout.has_m.then(|| ordinates.next()).flatten(),
        })
    }

    fn coords(&mut self) -> Result<Vec<Coord<f64>>, String> {
        let count = self.reader.count()?;
        let mut coords = Vec::with_capacity(count);
        for _ in 0..count {
            coords.push(self.coord()?);
        }
        Ok(coords)
    }

    /// A line, which `name` says is too short if it has one point.
    fn line(&mut self, name: &str) -> Result<LineString<f64>, String> {
        let coords = self.coords()?;
        if coords.len() == 1 {
            return Err(format!("{name} must have at least two points"));
        }
        Ok(LineString::new(coords, self.layout.dim))
    }

    fn polygon(&mut self) -> Result<Polygon<f64>, String> {
        let count = self.reader.count()?;
        let mut rings = Vec::with_capacity(count);
        for _ in 0..count {
            let mut coords = self.coords()?;
            // A ring is closed in X and Y, whatever its Z and M.
            if let (Some(first), Some(last)) = (coords.first(), coords.last())
                && (first.x, first.y) != (last.x, last.y)
            {
                coords.push(*first);
            }
            if coords.len() < 4 {
                return Err("Polygon must have at least four points in each ring".to_string());
            }
            rings.push(LineString::new(coords, self.layout.dim));
        }
        Ok(Polygon::new(rings, self.layout.dim))
    }
}
