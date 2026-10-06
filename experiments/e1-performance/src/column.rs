//! The unified loop style's column objects (README "Row access"; G2 §4; G3 §4) and output
//! builders shared by both styles.

use std::borrow::Cow;
use std::sync::Arc;

use arrow_array::{Array, ArrayRef};
use arrow_schema::Field;
use datafusion::error::Result;
use datafusion::logical_expr::ColumnarValue;
use geo_traits::GeometryTrait;
use arrow_array::builder::BinaryBuilder;
use geoarrow_array::array::{GenericWkbArray, WkbArray, from_arrow_array};
use geoarrow_array::builder::{GeometryBuilder, PointBuilder, PolygonBuilder, WkbBuilder};
use geoarrow_array::cast::to_wkb;
use geoarrow_array::GeoArrowArray;
use geoarrow_schema::{GeoArrowType, WkbType};

use crate::kernels::{GeoValue, ext, to_geo_value, to_geos};

/// The prototype from the task, unchanged apart from error types.
pub struct GeometryColumn {
    wkb: GenericWkbArray<i32>,
    is_scalar: bool,
}

impl GeometryColumn {
    pub fn try_new(value: &ColumnarValue, field: &Field) -> Result<Self> {
        let (array, is_scalar) = match value {
            ColumnarValue::Array(array) => (Arc::clone(array), false),
            ColumnarValue::Scalar(scalar) => (scalar.to_array()?, true),
        };
        let geo_array = from_arrow_array(&array, field).map_err(ext)?;
        Ok(Self {
            wkb: if FAST_WKB.load(std::sync::atomic::Ordering::Relaxed) {
                fast_to_wkb(geo_array.as_ref())?
            } else {
                to_wkb::<i32>(geo_array.as_ref()).map_err(ext)?
            },
            is_scalar,
        })
    }

    pub fn is_scalar(&self) -> bool {
        self.is_scalar
    }

    #[inline]
    pub fn get(&self, i: usize) -> Result<Option<wkb::reader::Wkb<'_>>> {
        let i = if self.is_scalar { 0 } else { i };
        let binary = self.wkb.inner();
        if binary.is_null(i) {
            return Ok(None);
        }
        Ok(Some(wkb::reader::read_wkb(binary.value(i)).map_err(ext)?))
    }
}

/// G2's column: a constant is converted to `geo` once; array rows are converted on `get`.
pub struct GeoColumn {
    column: GeometryColumn,
    scalar: Option<GeoValue>,
}

impl GeoColumn {
    pub fn try_new(value: &ColumnarValue, field: &Field) -> Result<Self> {
        let column = GeometryColumn::try_new(value, field)?;
        let scalar = if column.is_scalar() {
            Some(match column.get(0)? {
                Some(g) => to_geo_value(&g)?,
                None => GeoValue::Null,
            })
        } else {
            None
        };
        Ok(Self { column, scalar })
    }

    pub fn as_scalar(&self) -> Option<&GeoValue> {
        self.scalar.as_ref()
    }

    #[inline]
    pub fn get(&self, i: usize) -> Result<Cow<'_, GeoValue>> {
        if let Some(s) = &self.scalar {
            return Ok(Cow::Borrowed(s));
        }
        Ok(Cow::Owned(match self.column.get(i)? {
            Some(g) => to_geo_value(&g)?,
            None => GeoValue::Null,
        }))
    }
}

/// G3's column: every row converted to GEOS up front (a constant once).
pub struct GeosColumn {
    geometries: Vec<Option<geos::Geometry>>,
    is_scalar: bool,
}

impl GeosColumn {
    pub fn try_new(value: &ColumnarValue, field: &Field, number_rows: usize) -> Result<Self> {
        let column = GeometryColumn::try_new(value, field)?;
        let n = if column.is_scalar() { 1 } else { number_rows };
        let mut geometries = Vec::with_capacity(n);
        for i in 0..n {
            geometries.push(match column.get(i)? {
                Some(g) => Some(to_geos(&g)?),
                None => None,
            });
        }
        Ok(Self {
            geometries,
            is_scalar: column.is_scalar(),
        })
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<&geos::Geometry> {
        let i = if self.is_scalar { 0 } else { i };
        self.geometries[i].as_ref()
    }
}

/// Geometry output: native (Point, Polygon or Geometry) or WKB, chosen from the return field.
pub enum GeomOut {
    Point(PointBuilder),
    Polygon(PolygonBuilder),
    Geometry(GeometryBuilder),
    Wkb(WkbBuilder<i32>),
}

impl GeomOut {
    pub fn try_new(return_field: &Field, capacity: usize) -> Result<Self> {
        Ok(match GeoArrowType::from_arrow_field(return_field).map_err(ext)? {
            GeoArrowType::Point(t) => GeomOut::Point(PointBuilder::with_capacity(t, capacity)),
            GeoArrowType::Polygon(t) => GeomOut::Polygon(PolygonBuilder::new(t)),
            GeoArrowType::Geometry(t) => GeomOut::Geometry(GeometryBuilder::new(t)),
            GeoArrowType::Wkb(t) => GeomOut::Wkb(WkbBuilder::new(t)),
            other => panic!("unsupported output type {other:?}"),
        })
    }

    #[inline]
    pub fn push(&mut self, g: &impl GeometryTrait<T = f64>) -> Result<()> {
        match self {
            GeomOut::Point(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Polygon(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Geometry(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Wkb(b) => b.push_geometry(Some(g)).map_err(ext),
        }
    }

    #[inline]
    pub fn push_null(&mut self) {
        match self {
            GeomOut::Point(b) => b.push_null(),
            GeomOut::Polygon(b) => b.push_polygon(None::<&geo::Polygon>).unwrap(),
            GeomOut::Geometry(b) => b.push_null(),
            GeomOut::Wkb(b) => b.push_geometry(None::<&geo::Geometry>).unwrap(),
        }
    }

    /// For EMPTY input: an empty point for point output, otherwise NULL (not exercised: the
    /// inputs have no EMPTY rows).
    pub fn push_empty(&mut self) {
        match self {
            GeomOut::Point(b) => b.push_empty(),
            _ => self.push_null(),
        }
    }

    pub fn finish(self) -> ArrayRef {
        match self {
            GeomOut::Point(b) => b.finish().into_array_ref(),
            GeomOut::Polygon(b) => b.finish().into_array_ref(),
            GeomOut::Geometry(b) => b.finish().into_array_ref(),
            GeomOut::Wkb(b) => b.finish().into_array_ref(),
        }
    }
}

/// G3's `GeosGeometryBuilder`: GEOS → WKBWriter → `read_wkb` → GeoArrow builder.
/// With WKB output the GEOS WKB bytes are appended as they are.
pub enum GeosOut {
    Native(GeomOut),
    Wkb(BinaryBuilder, WkbType),
}

pub struct GeosGeometryBuilder {
    out: GeosOut,
    writer: geos::WKBWriter,
}

impl GeosGeometryBuilder {
    pub fn try_new(return_field: &Field, capacity: usize) -> Result<Self> {
        let mut writer = geos::WKBWriter::new().map_err(ext)?;
        writer.set_output_dimension(geos::CoordDimensions::ThreeD);
        let out = match GeoArrowType::from_arrow_field(return_field).map_err(ext)? {
            GeoArrowType::Wkb(t) => GeosOut::Wkb(BinaryBuilder::with_capacity(capacity, 0), t),
            _ => GeosOut::Native(GeomOut::try_new(return_field, capacity)?),
        };
        Ok(Self { out, writer })
    }

    #[inline]
    pub fn push_geos(&mut self, g: &geos::Geometry) -> Result<()> {
        let bytes = self.writer.write_wkb(g).map_err(ext)?;
        match &mut self.out {
            GeosOut::Wkb(b, _) => {
                b.append_value(&bytes);
                Ok(())
            }
            GeosOut::Native(out) => {
                let w = wkb::reader::read_wkb(&bytes).map_err(ext)?;
                out.push(&w)
            }
        }
    }

    pub fn push_null(&mut self) {
        match &mut self.out {
            GeosOut::Wkb(b, _) => b.append_null(),
            GeosOut::Native(out) => out.push_null(),
        }
    }

    pub fn finish(self) -> ArrayRef {
        match self.out {
            GeosOut::Wkb(mut b, t) => WkbArray::new(b.finish(), t.metadata().clone()).into_array_ref(),
            GeosOut::Native(out) => out.finish(),
        }
    }
}

pub fn array_of(value: &ColumnarValue, number_rows: usize) -> Result<ArrayRef> {
    Ok(match value {
        ColumnarValue::Array(a) => Arc::clone(a),
        ColumnarValue::Scalar(s) => s.to_array_of_size(number_rows)?,
    })
}

pub fn geo_array(value: &ColumnarValue, field: &Field, number_rows: usize) -> Result<Arc<dyn GeoArrowArray>> {
    let a = array_of(value, number_rows)?;
    from_arrow_array(a.as_ref(), field).map_err(ext)
}

/// Sensitivity variant (`v_` UDFs): a direct native→WKB writer for XY Point and Polygon arrays,
/// copying coordinates from the buffers instead of going through geo-traits and `wkb::writer`.
/// Everything else falls back to `to_wkb`.
pub static FAST_WKB: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn fast_to_wkb(arr: &dyn GeoArrowArray) -> Result<GenericWkbArray<i32>> {
    use arrow_array::BinaryArray;
    use arrow_buffer::{Buffer, OffsetBuffer, ScalarBuffer};
    use geoarrow_array::array::CoordBuffer;
    use geoarrow_array::cast::AsGeoArrowArray;
    use geoarrow_schema::Dimension;

    fn xy(coords: &CoordBuffer) -> Option<(&[f64], Option<&[f64]>)> {
        if coords.dim() != Dimension::XY {
            return None;
        }
        Some(match coords {
            CoordBuffer::Interleaved(c) => (c.coords().as_ref(), None),
            CoordBuffer::Separated(c) => {
                let b = c.raw_buffers();
                (b[0].as_ref(), Some(b[1].as_ref()))
            }
        })
    }
    #[inline(always)]
    fn push_coord(out: &mut Vec<u8>, c: (&[f64], Option<&[f64]>), j: usize) {
        let (x, y) = match c.1 {
            None => (c.0[2 * j], c.0[2 * j + 1]),
            Some(ys) => (c.0[j], ys[j]),
        };
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }

    let (values, offsets, nulls, metadata) = match arr.data_type() {
        GeoArrowType::Point(t) => {
            let a = arr.as_point();
            let Some(c) = xy(a.coords()) else { return to_wkb::<i32>(arr).map_err(ext) };
            let n = a.len();
            let mut out = Vec::with_capacity(n * 21);
            let mut offs = Vec::with_capacity(n + 1);
            offs.push(0i32);
            for j in 0..n {
                if a.is_valid(j) {
                    out.push(1u8);
                    out.extend_from_slice(&1u32.to_le_bytes());
                    push_coord(&mut out, c, j);
                }
                offs.push(out.len() as i32);
            }
            (out, offs, a.logical_nulls(), t.metadata().clone())
        }
        GeoArrowType::Polygon(t) => {
            let a = arr.as_polygon();
            let Some(c) = xy(a.coords()) else { return to_wkb::<i32>(arr).map_err(ext) };
            let geom_offsets = a.geom_offsets();
            let ring_offsets = a.ring_offsets();
            let n = a.len();
            let ncoords = *ring_offsets.last().unwrap() as usize;
            let mut out = Vec::with_capacity(n * 9 + ring_offsets.len() * 4 + ncoords * 16);
            let mut offs = Vec::with_capacity(n + 1);
            offs.push(0i32);
            for j in 0..n {
                if a.is_valid(j) {
                    let (r0, r1) = (geom_offsets[j] as usize, geom_offsets[j + 1] as usize);
                    out.push(1u8);
                    out.extend_from_slice(&3u32.to_le_bytes());
                    out.extend_from_slice(&((r1 - r0) as u32).to_le_bytes());
                    for r in r0..r1 {
                        let (c0, c1) = (ring_offsets[r] as usize, ring_offsets[r + 1] as usize);
                        out.extend_from_slice(&((c1 - c0) as u32).to_le_bytes());
                        for k in c0..c1 {
                            push_coord(&mut out, c, k);
                        }
                    }
                }
                offs.push(out.len() as i32);
            }
            (out, offs, a.logical_nulls(), t.metadata().clone())
        }
        _ => return to_wkb::<i32>(arr).map_err(ext),
    };
    let binary = BinaryArray::new(
        OffsetBuffer::new(ScalarBuffer::from(offsets)),
        Buffer::from_vec(values),
        nulls,
    );
    Ok(GenericWkbArray::new(binary, metadata))
}
