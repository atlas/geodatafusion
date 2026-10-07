//! Bounding boxes of geometries, as PostGIS computes them for box2d and box3d.
//!
//! Note: This started as a copy of the bounds code in the geoparquet crate.

use geo_traits::{
    CoordTrait, GeometryCollectionTrait, GeometryTrait, GeometryType, LineStringTrait, LineTrait,
    MultiLineStringTrait, MultiPointTrait, MultiPolygonTrait, PointTrait, PolygonTrait, RectTrait,
    TriangleTrait, UnimplementedGeometryCollection, UnimplementedLine, UnimplementedLineString,
    UnimplementedMultiLineString, UnimplementedMultiPoint, UnimplementedMultiPolygon,
    UnimplementedPoint, UnimplementedPolygon, UnimplementedTriangle,
};
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor, downcast_geoarrow_array};
use geoarrow_schema::error::GeoArrowResult;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::ordinates::z;

/// The bounding box of the coordinates added to it.
///
/// A box nothing was added to is empty: PostGIS has no box for an EMPTY geometry. M never counts.
#[derive(Debug, Clone, Copy)]
pub struct BoundingRect {
    pub(crate) minx: f64,
    pub(crate) miny: f64,
    pub(crate) minz: f64,
    pub(crate) maxx: f64,
    pub(crate) maxy: f64,
    pub(crate) maxz: f64,
    /// If `true`, expose itself as a 3D box through geo-traits, with Z 0 when no coordinate had
    /// a Z, as PostGIS's box3d does. Otherwise 2D. The GeoArrow builders need the dimension of
    /// every box to match the column's.
    include_z: bool,
}

impl BoundingRect {
    pub fn new(include_z: bool) -> Self {
        BoundingRect {
            minx: f64::INFINITY,
            miny: f64::INFINITY,
            minz: f64::INFINITY,
            maxx: -f64::INFINITY,
            maxy: -f64::INFINITY,
            maxz: -f64::INFINITY,
            include_z,
        }
    }

    /// The raw bounds, `[minx, miny, minz, maxx, maxy, maxz]`, with ±infinity for bounds nothing
    /// was added to.
    pub fn raw_bounds(&self) -> [f64; 6] {
        [
            self.minx, self.miny, self.minz, self.maxx, self.maxy, self.maxz,
        ]
    }

    /// A box with the raw bounds of [`Self::raw_bounds`].
    pub fn from_raw_bounds(state: [f64; 6], include_z: bool) -> Self {
        let [minx, miny, minz, maxx, maxy, maxz] = state;
        BoundingRect {
            minx,
            miny,
            minz,
            maxx,
            maxy,
            maxz,
            include_z,
        }
    }

    /// Whether no coordinate was added.
    pub fn is_empty(&self) -> bool {
        self.minx > self.maxx
    }

    pub fn minx(&self) -> f64 {
        self.minx
    }

    pub fn miny(&self) -> f64 {
        self.miny
    }

    /// The smallest Z, or 0 if no coordinate had a Z.
    pub fn minz(&self) -> f64 {
        if self.minz > self.maxz {
            0.0
        } else {
            self.minz
        }
    }

    pub fn maxx(&self) -> f64 {
        self.maxx
    }

    pub fn maxy(&self) -> f64 {
        self.maxy
    }

    /// The largest Z, or 0 if no coordinate had a Z.
    pub fn maxz(&self) -> f64 {
        if self.minz > self.maxz {
            0.0
        } else {
            self.maxz
        }
    }

    fn add_coord(&mut self, coord: &impl CoordTrait<T = f64>) {
        self.minx = self.minx.min(coord.x());
        self.miny = self.miny.min(coord.y());
        self.maxx = self.maxx.max(coord.x());
        self.maxy = self.maxy.max(coord.y());
        if let Some(z) = z(coord) {
            self.minz = self.minz.min(z);
            self.maxz = self.maxz.max(z);
        }
    }

    fn add_line_string(&mut self, line_string: &impl LineStringTrait<T = f64>) {
        for coord in line_string.coords() {
            self.add_coord(&coord);
        }
    }

    fn add_polygon(&mut self, polygon: &impl PolygonTrait<T = f64>) {
        if let Some(exterior) = polygon.exterior() {
            self.add_line_string(&exterior);
        }
        for interior in polygon.interiors() {
            self.add_line_string(&interior);
        }
    }

    pub(crate) fn add_geometry(&mut self, geometry: &impl GeometryTrait<T = f64>) {
        use GeometryType::*;

        match geometry.as_type() {
            Point(point) => {
                if let Some(coord) = point.coord() {
                    self.add_coord(&coord);
                }
            }
            LineString(line_string) => self.add_line_string(line_string),
            Polygon(polygon) => self.add_polygon(polygon),
            MultiPoint(points) => points.points().for_each(|point| self.add_geometry(&point)),
            MultiLineString(line_strings) => line_strings
                .line_strings()
                .for_each(|line_string| self.add_line_string(&line_string)),
            MultiPolygon(polygons) => polygons
                .polygons()
                .for_each(|polygon| self.add_polygon(&polygon)),
            GeometryCollection(collection) => collection
                .geometries()
                .for_each(|member| self.add_geometry(&member)),
            Rect(rect) => {
                self.add_coord(&rect.min());
                self.add_coord(&rect.max());
            }
            Triangle(triangle) => triangle
                .coords()
                .iter()
                .for_each(|coord| self.add_coord(coord)),
            Line(line) => {
                self.add_coord(&line.start());
                self.add_coord(&line.end());
            }
        }
    }

    /// Grows this box to cover `other`.
    pub fn update(&mut self, other: &BoundingRect) {
        self.minx = self.minx.min(other.minx);
        self.miny = self.miny.min(other.miny);
        self.minz = self.minz.min(other.minz);
        self.maxx = self.maxx.max(other.maxx);
        self.maxy = self.maxy.max(other.maxy);
        self.maxz = self.maxz.max(other.maxz);
    }
}

impl RectTrait for BoundingRect {
    type CoordType<'a> = Coord;

    fn min(&self) -> Self::CoordType<'_> {
        Coord {
            x: self.minx,
            y: self.miny,
            z: self.include_z.then(|| self.minz()),
            m: None,
        }
    }

    fn max(&self) -> Self::CoordType<'_> {
        Coord {
            x: self.maxx,
            y: self.maxy,
            z: self.include_z.then(|| self.maxz()),
            m: None,
        }
    }
}

impl GeometryTrait for BoundingRect {
    type T = f64;
    type PointType<'a>
        = UnimplementedPoint<f64>
    where
        Self: 'a;
    type LineStringType<'a>
        = UnimplementedLineString<f64>
    where
        Self: 'a;
    type PolygonType<'a>
        = UnimplementedPolygon<f64>
    where
        Self: 'a;
    type MultiPointType<'a>
        = UnimplementedMultiPoint<f64>
    where
        Self: 'a;
    type MultiLineStringType<'a>
        = UnimplementedMultiLineString<f64>
    where
        Self: 'a;
    type MultiPolygonType<'a>
        = UnimplementedMultiPolygon<f64>
    where
        Self: 'a;
    type GeometryCollectionType<'a>
        = UnimplementedGeometryCollection<f64>
    where
        Self: 'a;
    type RectType<'a>
        = Self
    where
        Self: 'a;
    type TriangleType<'a>
        = UnimplementedTriangle<f64>
    where
        Self: 'a;
    type LineType<'a>
        = UnimplementedLine<f64>
    where
        Self: 'a;

    fn dim(&self) -> geo_traits::Dimensions {
        if self.include_z {
            geo_traits::Dimensions::Xyz
        } else {
            geo_traits::Dimensions::Xy
        }
    }

    fn as_type(
        &self,
    ) -> GeometryType<
        '_,
        Self::PointType<'_>,
        Self::LineStringType<'_>,
        Self::PolygonType<'_>,
        Self::MultiPointType<'_>,
        Self::MultiLineStringType<'_>,
        Self::MultiPolygonType<'_>,
        Self::GeometryCollectionType<'_>,
        Self::RectType<'_>,
        Self::TriangleType<'_>,
        Self::LineType<'_>,
    > {
        GeometryType::Rect(self)
    }
}

/// The bounding box of each geometry; NULL for an EMPTY geometry, as in PostGIS.
///
/// Note that this is fully planar and **does not** handle the antimeridian for geographic
/// coordinates.
pub(crate) struct BoundsKernel {
    pub(crate) include_z: bool,
}

impl GeometryKernel for BoundsKernel {
    type Output = BoundingRect;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<BoundingRect>> {
        let mut rect = BoundingRect::new(self.include_z);
        rect.add_geometry(geom);
        Ok((!rect.is_empty()).then_some(rect))
    }
}

/// The bounds of every geometry of the array, skipping NULL and EMPTY ones.
///
/// With `include_z`, each geometry contributes its box3d, so a geometry without Z counts as Z 0,
/// as in PostGIS's ST_3DExtent.
pub(crate) fn extent_bounds(
    arr: &dyn GeoArrowArray,
    include_z: bool,
) -> GeoDataFusionResult<BoundingRect> {
    let kernel = BoundsKernel { include_z };
    let rects: Vec<Option<BoundingRect>> = map_geometry(arr, &kernel)?;
    let mut total = BoundingRect::new(include_z);
    for rect in rects.iter().flatten() {
        let [minx, miny, _, maxx, maxy, _] = rect.raw_bounds();
        total.update(&BoundingRect::from_raw_bounds(
            [minx, miny, rect.minz(), maxx, maxy, rect.maxz()],
            include_z,
        ));
    }
    Ok(total)
}

/// Get the total bounds (i.e. minx, miny, maxx, maxy) of the entire geoarrow array.
pub fn total_bounds(arr: &dyn GeoArrowArray) -> GeoArrowResult<BoundingRect> {
    downcast_geoarrow_array!(arr, impl_total_bounds)
}

/// The actual implementation of computing the total bounds
fn impl_total_bounds<'a>(arr: &'a impl GeoArrowArrayAccessor<'a>) -> GeoArrowResult<BoundingRect> {
    let mut rect = BoundingRect::new(false);

    for item in arr.iter().flatten() {
        rect.add_geometry(&item?);
    }

    Ok(rect)
}
