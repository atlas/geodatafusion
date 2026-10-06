//! Deterministic input data.

use std::f64::consts::TAU;
use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{Field, Schema};
use geo::{Coord, LineString, Point, Polygon};
use geoarrow_array::GeoArrowArray;

use geoarrow_array::builder::{PointBuilder, PolygonBuilder, WkbBuilder};
use geoarrow_array::capacity::PolygonCapacity;
use geoarrow_schema::{CoordType, Dimension, PointType, PolygonType, WkbType};

/// splitmix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    /// Points uniform in [0, 1000)².
    Points,
    /// Star-shaped simple polygons with `n` vertices (n + 1 coordinates, closed), radius ~1.
    Polygons(usize),
}

impl Dataset {
    pub fn parse(s: &str) -> Self {
        match s {
            "points" => Dataset::Points,
            _ => Dataset::Polygons(
                s.strip_prefix("poly")
                    .and_then(|n| n.parse().ok())
                    .expect("dataset: points | poly<N>"),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Separated,
    Interleaved,
    Wkb,
}

impl Encoding {
    pub fn parse(s: &str) -> Self {
        match s {
            "sep" => Encoding::Separated,
            "int" => Encoding::Interleaved,
            "wkb" => Encoding::Wkb,
            _ => panic!("encoding: sep | int | wkb"),
        }
    }
}

/// A star-shaped polygon: monotonically increasing angles, so it's always simple and valid.
pub fn star_polygon(rng: &mut Rng, cx: f64, cy: f64, radius: f64, n: usize) -> Polygon {
    let mut coords = Vec::with_capacity(n + 1);
    for i in 0..n {
        let angle = (i as f64 + 0.8 * rng.f64()) / n as f64 * TAU;
        let r = radius * (0.7 + 0.3 * rng.f64());
        coords.push(Coord {
            x: cx + r * angle.cos(),
            y: cy + r * angle.sin(),
        });
    }
    coords.push(coords[0]);
    Polygon::new(LineString::new(coords), vec![])
}

/// The constant polygon for ST_Intersects: 100 vertices, centred, radius ~300 of the 1000 extent.
pub fn constant_polygon() -> Polygon {
    let mut rng = Rng::new(42);
    star_polygon(&mut rng, 500.0, 500.0, 330.0, 100)
}

pub fn polygon_wkt(p: &Polygon) -> String {
    let mut s = String::from("POLYGON((");
    for (i, c) in p.exterior().coords().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("{:.17} {:.17}", c.x, c.y));
    }
    s.push_str("))");
    s
}

/// One geometry column `geom`, split into batches of `batch_size` rows.
pub fn make_batches(
    dataset: Dataset,
    encoding: Encoding,
    rows: usize,
    batch_size: usize,
    with_constant_column: bool,
) -> (Arc<Schema>, Vec<RecordBatch>) {
    let mut rng = Rng::new(0xE1);
    let mut batches = Vec::new();
    let mut schema = None;
    let constant = constant_polygon();
    let mut start = 0;
    while start < rows {
        let len = batch_size.min(rows - start);
        let (field, array) = match dataset {
            Dataset::Points => {
                let points: Vec<Point> = (0..len)
                    .map(|_| Point::new(rng.f64() * 1000.0, rng.f64() * 1000.0))
                    .collect();
                encode_points(&points, encoding)
            }
            Dataset::Polygons(n) => {
                let polygons: Vec<Polygon> = (0..len)
                    .map(|_| {
                        let cx = rng.f64() * 1000.0;
                        let cy = rng.f64() * 1000.0;
                        star_polygon(&mut rng, cx, cy, 1.0, n)
                    })
                    .collect();
                encode_polygons(&polygons, encoding)
            }
        };
        let mut fields = vec![field];
        let mut columns = vec![array];
        if with_constant_column {
            // The constant polygon materialized as a column, for array-array ST_Intersects.
            let (f, a) = encode_polygons(&vec![constant.clone(); len], encoding);
            fields.push(Arc::new(f.as_ref().clone().with_name("q")));
            columns.push(a);
        }
        let s = schema
            .get_or_insert_with(|| Arc::new(Schema::new(fields.clone())))
            .clone();
        batches.push(RecordBatch::try_new(s, columns).unwrap());
        start += len;
    }
    (schema.unwrap(), batches)
}

fn encode_points(points: &[Point], encoding: Encoding) -> (Arc<Field>, ArrayRef) {
    match encoding {
        Encoding::Separated | Encoding::Interleaved => {
            let ct = if encoding == Encoding::Separated {
                CoordType::Separated
            } else {
                CoordType::Interleaved
            };
            let typ = PointType::new(Dimension::XY, Default::default()).with_coord_type(ct);
            let mut b = PointBuilder::with_capacity(typ, points.len());
            for p in points {
                b.push_point(Some(p));
            }
            let arr = b.finish();
            (
                Arc::new(arr.data_type().to_field("geom", true)),
                arr.into_array_ref(),
            )
        }
        Encoding::Wkb => wkb_of(points.iter()),
    }
}

fn encode_polygons(polygons: &[Polygon], encoding: Encoding) -> (Arc<Field>, ArrayRef) {
    match encoding {
        Encoding::Separated | Encoding::Interleaved => {
            let ct = if encoding == Encoding::Separated {
                CoordType::Separated
            } else {
                CoordType::Interleaved
            };
            let typ = PolygonType::new(Dimension::XY, Default::default()).with_coord_type(ct);
            let cap = PolygonCapacity::from_polygons(polygons.iter().map(Some));
            let mut b = PolygonBuilder::with_capacity(typ, cap);
            for p in polygons {
                b.push_polygon(Some(p)).unwrap();
            }
            let arr = b.finish();
            (
                Arc::new(arr.data_type().to_field("geom", true)),
                arr.into_array_ref(),
            )
        }
        Encoding::Wkb => wkb_of(polygons.iter()),
    }
}

fn wkb_of<'a, G: geo_traits::GeometryTrait<T = f64> + 'a>(
    geoms: impl Iterator<Item = &'a G>,
) -> (Arc<Field>, ArrayRef) {
    let typ = WkbType::new(Default::default());
    let mut b = WkbBuilder::<i32>::new(typ);
    for g in geoms {
        b.push_geometry(Some(g)).unwrap();
    }
    let arr = b.finish();
    (
        Arc::new(arr.data_type().to_field("geom", true)),
        arr.into_array_ref(),
    )
}
