//! A local `geoarrow.geometry` (dense union) builder over geoarrow-array's public child builders.
//! It differs from `GeometryBuilder` in one way: a GEOMETRYCOLLECTION is always stored in the
//! GeometryCollection child, whatever its member count (no one-member collapse). Nulls go to the
//! child of the previously pushed type (XY Point before any value). Shared by the H2f round trip
//! and the H2d benchmark (`_l` variants).

use std::sync::Arc;

use arrow_buffer::ScalarBuffer;
use geo_traits::{GeometryTrait, GeometryType as G};
use geoarrow_array::array::GeometryArray;
use geoarrow_array::builder::{
    GeometryCollectionBuilder, LineStringBuilder, MultiLineStringBuilder, MultiPointBuilder,
    MultiPolygonBuilder, PointBuilder, PolygonBuilder,
};
use geoarrow_schema::error::{GeoArrowError, GeoArrowResult};
use geoarrow_schema::{
    CoordType, Dimension, GeometryCollectionType, GeometryType, LineStringType, Metadata,
    MultiLineStringType, MultiPointType, MultiPolygonType, PointType, PolygonType,
};

pub struct LocalGeometryBuilder {
    metadata: Arc<Metadata>,
    type_ids: Vec<i8>,
    offsets: Vec<i32>,
    points: [PointBuilder; 4],
    lines: [LineStringBuilder; 4],
    polygons: [PolygonBuilder; 4],
    mpoints: [MultiPointBuilder; 4],
    mlines: [MultiLineStringBuilder; 4],
    mpolygons: [MultiPolygonBuilder; 4],
    gcs: [GeometryCollectionBuilder; 4],
    lens: [[usize; 4]; 7], // child lengths (the builders' `len` is crate-private)
    last: (usize, usize), // (child kind 0..7, dimension order) of the last push, for nulls
}

fn order(d: Dimension) -> usize {
    match d {
        Dimension::XY => 0,
        Dimension::XYZ => 1,
        Dimension::XYM => 2,
        Dimension::XYZM => 3,
    }
}

const DIMS: [Dimension; 4] = [Dimension::XY, Dimension::XYZ, Dimension::XYM, Dimension::XYZM];

fn dims<T>(f: impl Fn(Dimension) -> T) -> [T; 4] {
    DIMS.map(f)
}

impl LocalGeometryBuilder {
    pub fn new(typ: GeometryType) -> Self {
        let ct = typ.coord_type();
        let m = || Default::default();
        Self {
            metadata: typ.metadata().clone(),
            type_ids: Vec::new(),
            offsets: Vec::new(),
            points: dims(|d| PointBuilder::new(PointType::new(d, m()).with_coord_type(ct))),
            lines: dims(|d| LineStringBuilder::new(LineStringType::new(d, m()).with_coord_type(ct))),
            polygons: dims(|d| PolygonBuilder::new(PolygonType::new(d, m()).with_coord_type(ct))),
            mpoints: dims(|d| MultiPointBuilder::new(MultiPointType::new(d, m()).with_coord_type(ct))),
            mlines: dims(|d| {
                MultiLineStringBuilder::new(MultiLineStringType::new(d, m()).with_coord_type(ct))
            }),
            mpolygons: dims(|d| {
                MultiPolygonBuilder::new(MultiPolygonType::new(d, m()).with_coord_type(ct))
            }),
            gcs: dims(|d| {
                GeometryCollectionBuilder::new(GeometryCollectionType::new(d, m()).with_coord_type(ct))
            }),
            lens: [[0; 4]; 7],
            last: (0, 0),
        }
    }

    fn record(&mut self, kind: usize, d: usize) {
        self.type_ids.push((10 * d + kind + 1) as i8);
        self.offsets.push(self.lens[kind][d] as i32);
        self.lens[kind][d] += 1;
        self.last = (kind, d);
    }

    #[inline]
    pub fn push_geometry(&mut self, g: Option<&impl GeometryTrait<T = f64>>) -> GeoArrowResult<()> {
        let Some(g) = g else {
            let (kind, d) = self.last;
            self.record(kind, d);
            match kind {
                0 => self.points[d].push_null(),
                1 => self.lines[d].push_line_string(None::<&geo_types::LineString>)?,
                2 => self.polygons[d].push_polygon(None::<&geo_types::Polygon>)?,
                3 => self.mpoints[d].push_multi_point(None::<&geo_types::MultiPoint>)?,
                4 => self.mlines[d].push_multi_line_string(None::<&geo_types::MultiLineString>)?,
                5 => self.mpolygons[d].push_multi_polygon(None::<&geo_types::MultiPolygon>)?,
                _ => self.gcs[d].push_geometry_collection(None::<&geo_types::GeometryCollection>)?,
            }
            return Ok(());
        };
        let d = order(
            Dimension::try_from(g.dim())
                .map_err(|_| GeoArrowError::InvalidGeoArrow("unknown dimension".into()))?,
        );
        match g.as_type() {
            G::Point(p) => {
                self.record(0, d);
                self.points[d].push_point(Some(p));
            }
            G::LineString(l) => {
                self.record(1, d);
                self.lines[d].push_line_string(Some(l))?;
            }
            G::Polygon(p) => {
                self.record(2, d);
                self.polygons[d].push_polygon(Some(p))?;
            }
            G::MultiPoint(p) => {
                self.record(3, d);
                self.mpoints[d].push_multi_point(Some(p))?;
            }
            G::MultiLineString(p) => {
                self.record(4, d);
                self.mlines[d].push_multi_line_string(Some(p))?;
            }
            G::MultiPolygon(p) => {
                self.record(5, d);
                self.mpolygons[d].push_multi_polygon(Some(p))?;
            }
            G::GeometryCollection(gc) => {
                self.record(6, d);
                self.gcs[d].push_geometry_collection(Some(gc))?;
            }
            // Rect/Triangle/Line: same conversions as GeometryBuilder, via the child builders.
            G::Rect(_) | G::Triangle(_) => {
                self.record(2, d);
                self.polygons[d].push_geometry(Some(g))?;
            }
            G::Line(_) => {
                self.record(1, d);
                self.lines[d].push_geometry(Some(g))?;
            }
        }
        Ok(())
    }

    pub fn finish(self) -> GeometryArray {
        GeometryArray::new(
            ScalarBuffer::from(self.type_ids),
            ScalarBuffer::from(self.offsets),
            self.points.map(|b| b.finish()),
            self.lines.map(|b| b.finish()),
            self.polygons.map(|b| b.finish()),
            self.mpoints.map(|b| b.finish()),
            self.mlines.map(|b| b.finish()),
            self.mpolygons.map(|b| b.finish()),
            self.gcs.map(|b| b.finish()),
            self.metadata,
        )
    }
}

#[allow(dead_code)]
pub fn separated() -> GeometryType {
    GeometryType::new(Default::default()).with_coord_type(CoordType::Separated)
}
