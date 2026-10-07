//! (E)WKB encoding, matching PostGIS.
//!
//! ISO WKB marks Z and M by adding 1000, 2000 or 3000 to the type code. PostGIS's EWKB sets
//! flag bits instead (Z 0x80000000, M 0x40000000) and can carry an SRID (flag 0x20000000,
//! followed by the SRID), which only the outermost geometry has. Both write an EMPTY point as
//! NaN coordinates. The output is checked against PostGIS by the test data in
//! `testdata/wkb.txt`.

use datafusion::common::exec_datafusion_err;
use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait,
};
use wkb::Endianness;
use wkb::writer::{WriteOptions, write_geometry};

use crate::error::GeoDataFusionResult;

const EWKB_Z: u32 = 0x8000_0000;
const EWKB_M: u32 = 0x4000_0000;
const EWKB_SRID: u32 = 0x2000_0000;

/// The byte order PostGIS's `NDRorXDR` argument selects: big-endian (XDR) for `'XDR'` or
/// `'xdr'`, little-endian (NDR) for anything else.
pub(crate) fn parse_endianness(text: &str) -> Endianness {
    if text == "XDR" || text == "xdr" {
        Endianness::BigEndian
    } else {
        Endianness::LittleEndian
    }
}

/// Appends the ISO WKB of a geometry.
pub(crate) fn write_wkb(
    out: &mut Vec<u8>,
    geom: &impl GeometryTrait<T = f64>,
    endianness: Endianness,
) -> GeoDataFusionResult<()> {
    write_geometry(out, geom, &WriteOptions { endianness })
        .map_err(|e| exec_datafusion_err!("{e}"))?;
    Ok(())
}

/// Appends the EWKB of a geometry, with `srid` in the outermost header unless it's 0.
pub(crate) fn write_ewkb(
    out: &mut Vec<u8>,
    geom: &impl GeometryTrait<T = f64>,
    srid: i32,
    endianness: Endianness,
) {
    EwkbWriter { out, endianness }.geometry(geom, srid);
}

/// The SRID in an EWKB header, if it has one. `None` for ISO WKB and for input too short to
/// have a header; the WKB reader reports those.
pub(crate) fn ewkb_srid(buf: &[u8]) -> Option<i32> {
    let read_u32 = |bytes: &[u8]| -> Option<u32> {
        let bytes: [u8; 4] = bytes.try_into().ok()?;
        Some(match buf.first()? {
            0 => u32::from_be_bytes(bytes),
            _ => u32::from_le_bytes(bytes),
        })
    };
    let type_code = read_u32(buf.get(1..5)?)?;
    if type_code & EWKB_SRID == 0 {
        return None;
    }
    read_u32(buf.get(5..9)?).map(|srid| srid as i32)
}

struct EwkbWriter<'a> {
    out: &'a mut Vec<u8>,
    endianness: Endianness,
}

impl EwkbWriter<'_> {
    fn u32(&mut self, value: u32) {
        match self.endianness {
            Endianness::BigEndian => self.out.extend_from_slice(&value.to_be_bytes()),
            Endianness::LittleEndian => self.out.extend_from_slice(&value.to_le_bytes()),
        }
    }

    fn f64(&mut self, value: f64) {
        match self.endianness {
            Endianness::BigEndian => self.out.extend_from_slice(&value.to_be_bytes()),
            Endianness::LittleEndian => self.out.extend_from_slice(&value.to_le_bytes()),
        }
    }

    fn count(&mut self, count: usize) {
        self.u32(u32::try_from(count).expect("WKB counts fit in 32 bits"));
    }

    fn header(&mut self, type_code: u32, geom: &impl GeometryTrait<T = f64>, srid: i32) {
        self.out.push(match self.endianness {
            Endianness::BigEndian => 0,
            Endianness::LittleEndian => 1,
        });
        let size = geom.dim().size();
        let has_m = matches!(geom.dim(), geo_traits::Dimensions::Xym) || size == 4;
        let has_z = size == 4 || (size == 3 && !has_m);
        let mut code = type_code;
        if has_z {
            code |= EWKB_Z;
        }
        if has_m {
            code |= EWKB_M;
        }
        if srid != 0 {
            code |= EWKB_SRID;
        }
        self.u32(code);
        if srid != 0 {
            self.u32(srid as u32);
        }
    }

    /// The ordinates of a coordinate in storage order: X, Y, then Z and M as it has them.
    fn coord(&mut self, coord: &impl CoordTrait<T = f64>) {
        for i in 0..coord.dim().size() {
            self.f64(coord.nth_or_panic(i));
        }
    }

    fn coords<C: CoordTrait<T = f64>>(&mut self, coords: impl ExactSizeIterator<Item = C>) {
        self.count(coords.len());
        for coord in coords {
            self.coord(&coord);
        }
    }

    fn polygon_rings<L: LineStringTrait<T = f64>>(
        &mut self,
        exterior: Option<L>,
        interiors: impl ExactSizeIterator<Item = L>,
    ) {
        let Some(exterior) = exterior else {
            self.count(0);
            return;
        };
        self.count(1 + interiors.len());
        self.coords(exterior.coords());
        for interior in interiors {
            self.coords(interior.coords());
        }
    }

    fn geometry(&mut self, geom: &impl GeometryTrait<T = f64>, srid: i32) {
        match geom.as_type() {
            GeometryType::Point(point) => {
                self.header(1, geom, srid);
                match point.coord() {
                    Some(coord) => self.coord(&coord),
                    None => {
                        for _ in 0..geom.dim().size() {
                            self.f64(f64::NAN);
                        }
                    }
                }
            }
            GeometryType::LineString(line) => {
                self.header(2, geom, srid);
                self.coords(line.coords());
            }
            GeometryType::Polygon(polygon) => {
                self.header(3, geom, srid);
                self.polygon_rings(polygon.exterior(), polygon.interiors());
            }
            GeometryType::MultiPoint(points) => {
                self.header(4, geom, srid);
                self.count(points.num_points());
                for point in points.points() {
                    self.geometry(&point, 0);
                }
            }
            GeometryType::MultiLineString(lines) => {
                self.header(5, geom, srid);
                self.count(lines.num_line_strings());
                for line in lines.line_strings() {
                    self.geometry(&line, 0);
                }
            }
            GeometryType::MultiPolygon(polygons) => {
                self.header(6, geom, srid);
                self.count(polygons.num_polygons());
                for polygon in polygons.polygons() {
                    self.geometry(&polygon, 0);
                }
            }
            GeometryType::GeometryCollection(collection) => {
                self.header(7, geom, srid);
                self.count(collection.num_geometries());
                for member in collection.geometries() {
                    self.geometry(&member, 0);
                }
            }
            // As the polygon or linestring PostGIS would see.
            GeometryType::Rect(rect) => {
                self.header(3, geom, srid);
                let (min, max) = (rect.min(), rect.max());
                let ring = [
                    (min.x(), min.y()),
                    (max.x(), min.y()),
                    (max.x(), max.y()),
                    (min.x(), max.y()),
                    (min.x(), min.y()),
                ];
                self.count(1);
                self.count(ring.len());
                for (x, y) in ring {
                    self.f64(x);
                    self.f64(y);
                }
            }
            GeometryType::Triangle(triangle) => {
                self.header(3, geom, srid);
                let coords = triangle.coords();
                self.count(1);
                self.count(4);
                for coord in coords.iter().chain(coords.first()) {
                    self.coord(coord);
                }
            }
            GeometryType::Line(line) => {
                self.header(2, geom, srid);
                self.count(2);
                self.coord(&line.start());
                self.coord(&line.end());
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::udf::native::io::util::wkt::parse_ewkt;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02X}")).collect()
    }

    #[test]
    fn test_write_matches_postgis() {
        let cases = include_str!("testdata/wkb.txt");
        let mut mismatches = vec![];
        for line in cases.lines().filter(|line| !line.starts_with('#')) {
            let fields: Vec<&str> = line.split('|').collect();
            let [ewkt, endianness, expected_ewkb, expected_wkb] = fields[..] else {
                panic!("malformed test case {line:?}");
            };
            let (srid, geom) = parse_ewkt(ewkt).unwrap();
            let endianness = parse_endianness(endianness);
            let mut ewkb = vec![];
            write_ewkb(&mut ewkb, &geom, srid.unwrap_or(0), endianness);
            let mut wkb = vec![];
            write_wkb(&mut wkb, &geom, endianness).unwrap();
            let (ewkb, wkb) = (hex(&ewkb), hex(&wkb));
            if ewkb != expected_ewkb || wkb != expected_wkb {
                mismatches.push(format!("{line}\n  got {ewkb}|{wkb}"));
            }
            if ewkb_srid(&hex_decode(expected_ewkb)) != srid.filter(|srid| *srid != 0) {
                mismatches.push(format!("{line}\n  wrong ewkb_srid"));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} mismatches:\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }

    fn hex_decode(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn test_parse_endianness() {
        assert!(matches!(parse_endianness("XDR"), Endianness::BigEndian));
        assert!(matches!(parse_endianness("xdr"), Endianness::BigEndian));
        assert!(matches!(parse_endianness("xDr"), Endianness::LittleEndian));
        assert!(matches!(parse_endianness("NDR"), Endianness::LittleEndian));
    }
}
