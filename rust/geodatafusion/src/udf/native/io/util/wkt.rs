//! Well-Known Text writing, matching PostGIS.

use geo_traits::{
    CoordTrait, Dimensions, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait,
    LineTrait, MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait,
    RectTrait, TriangleTrait,
};

use crate::udf::native::io::util::number::write_number;

/// Writes ISO WKT as PostGIS's ST_AsText does: `POINT Z (1 2 3)`, `MULTIPOINT((1 2),(3 4))`,
/// `POINT Z EMPTY`, with every collection member tagged with its dimension.
pub(crate) fn write_wkt(
    out: &mut String,
    geom: &impl GeometryTrait<T = f64>,
    max_decimal_digits: i32,
) {
    let writer = WktWriter { max_decimal_digits };
    writer.geometry(out, geom);
}

struct WktWriter {
    max_decimal_digits: i32,
}

impl WktWriter {
    fn geometry(&self, out: &mut String, geom: &impl GeometryTrait<T = f64>) {
        let dim = geom.dim();
        match geom.as_type() {
            GeometryType::Point(point) => {
                self.header(out, "POINT", dim, is_empty_point(point));
                if let Some(coord) = point.coord().filter(|_| !is_empty_point(point)) {
                    out.push('(');
                    self.coord(out, &coord);
                    out.push(')');
                }
            }
            GeometryType::LineString(line) => {
                self.header(out, "LINESTRING", dim, line.num_coords() == 0);
                if line.num_coords() > 0 {
                    self.coords(out, line.coords());
                }
            }
            GeometryType::Polygon(polygon) => {
                let empty = polygon.exterior().is_none_or(|ring| ring.num_coords() == 0);
                self.header(out, "POLYGON", dim, empty);
                if !empty {
                    self.polygon_rings(out, polygon);
                }
            }
            GeometryType::MultiPoint(multi) => {
                self.header(out, "MULTIPOINT", dim, multi.num_points() == 0);
                if multi.num_points() > 0 {
                    out.push('(');
                    for (i, point) in multi.points().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        match point.coord().filter(|_| !is_empty_point(&point)) {
                            Some(coord) => {
                                out.push('(');
                                self.coord(out, &coord);
                                out.push(')');
                            }
                            None => out.push_str("EMPTY"),
                        }
                    }
                    out.push(')');
                }
            }
            GeometryType::MultiLineString(multi) => {
                self.header(out, "MULTILINESTRING", dim, multi.num_line_strings() == 0);
                if multi.num_line_strings() > 0 {
                    out.push('(');
                    for (i, line) in multi.line_strings().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        self.coords_or_empty(out, line.coords(), line.num_coords());
                    }
                    out.push(')');
                }
            }
            GeometryType::MultiPolygon(multi) => {
                self.header(out, "MULTIPOLYGON", dim, multi.num_polygons() == 0);
                if multi.num_polygons() > 0 {
                    out.push('(');
                    for (i, polygon) in multi.polygons().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        if polygon.exterior().is_none_or(|ring| ring.num_coords() == 0) {
                            out.push_str("EMPTY");
                        } else {
                            self.polygon_rings(out, &polygon);
                        }
                    }
                    out.push(')');
                }
            }
            GeometryType::GeometryCollection(collection) => {
                let empty = collection.num_geometries() == 0;
                self.header(out, "GEOMETRYCOLLECTION", dim, empty);
                if !empty {
                    out.push('(');
                    for (i, member) in collection.geometries().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        self.geometry(out, &member);
                    }
                    out.push(')');
                }
            }
            // Boxes are written as their polygon, as PostGIS does for box2d input.
            GeometryType::Rect(rect) => {
                self.header(out, "POLYGON", dim, false);
                let (min, max) = (rect.min(), rect.max());
                out.push_str("((");
                for (i, (x, y)) in [
                    (min.x(), min.y()),
                    (min.x(), max.y()),
                    (max.x(), max.y()),
                    (max.x(), min.y()),
                    (min.x(), min.y()),
                ]
                .into_iter()
                .enumerate()
                {
                    if i > 0 {
                        out.push(',');
                    }
                    write_number(out, x, self.max_decimal_digits);
                    out.push(' ');
                    write_number(out, y, self.max_decimal_digits);
                }
                out.push_str("))");
            }
            GeometryType::Triangle(triangle) => {
                self.header(out, "TRIANGLE", dim, false);
                out.push('(');
                self.coords(out, triangle.coords().into_iter().chain([triangle.first()]));
                out.push(')');
            }
            GeometryType::Line(line) => {
                self.header(out, "LINESTRING", dim, false);
                self.coords(out, line.coords().into_iter());
            }
        }
    }

    /// Writes the type keyword, the dimension tag and, for an empty geometry, `EMPTY`. A
    /// non-empty geometry with a tag gets a space before its coordinates (`POINT Z (`).
    fn header(&self, out: &mut String, keyword: &str, dim: Dimensions, empty: bool) {
        out.push_str(keyword);
        let tag = match dim {
            Dimensions::Xyz | Dimensions::Unknown(3) => " Z",
            Dimensions::Xym => " M",
            Dimensions::Xyzm | Dimensions::Unknown(4) => " ZM",
            _ => "",
        };
        out.push_str(tag);
        if empty {
            out.push_str(" EMPTY");
        } else if !tag.is_empty() {
            out.push(' ');
        }
    }

    fn polygon_rings(&self, out: &mut String, polygon: &impl PolygonTrait<T = f64>) {
        out.push('(');
        if let Some(exterior) = polygon.exterior() {
            self.coords(out, exterior.coords());
        }
        for interior in polygon.interiors() {
            out.push(',');
            self.coords(out, interior.coords());
        }
        out.push(')');
    }

    fn coords_or_empty<C: CoordTrait<T = f64>>(
        &self,
        out: &mut String,
        coords: impl Iterator<Item = C>,
        count: usize,
    ) {
        if count == 0 {
            out.push_str("EMPTY");
        } else {
            self.coords(out, coords);
        }
    }

    fn coords<C: CoordTrait<T = f64>>(&self, out: &mut String, coords: impl Iterator<Item = C>) {
        out.push('(');
        for (i, coord) in coords.enumerate() {
            if i > 0 {
                out.push(',');
            }
            self.coord(out, &coord);
        }
        out.push(')');
    }

    fn coord(&self, out: &mut String, coord: &impl CoordTrait<T = f64>) {
        for i in 0..coord.dim().size() {
            if i > 0 {
                out.push(' ');
            }
            write_number(out, coord.nth_or_panic(i), self.max_decimal_digits);
        }
    }
}

/// A point is empty without a coordinate, or with NaN coordinates (WKB's EMPTY point).
fn is_empty_point(point: &impl PointTrait<T = f64>) -> bool {
    point
        .coord()
        .is_none_or(|coord| coord.x().is_nan() && coord.y().is_nan())
}
