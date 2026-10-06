//! Geometry output builders: union-only (`geoarrow.geometry`, separated) or WKB.

use std::io::Write as _;
use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::builder::BinaryBuilder;
use arrow_schema::Field;
use datafusion::error::Result;
use datafusion::logical_expr::ColumnarValue;
use geo_traits::GeometryTrait;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{WkbArray, from_arrow_array};
use geoarrow_array::builder::{GeometryBuilder, WkbBuilder};
use geoarrow_schema::{GeoArrowType, GeometryType, Metadata, WkbType};

#[path = "../../shared/union_builder.rs"]
mod union_builder;
pub use union_builder::LocalGeometryBuilder;

use crate::kernels::ext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Out {
    /// `geoarrow.geometry` via geoarrow-array's `GeometryBuilder` (`_n`).
    Union,
    /// `geoarrow.geometry` via the local builder in `shared/union_builder.rs` (`_l`).
    UnionLocal,
    /// `geoarrow.wkb` via geoarrow-array's `WkbBuilder`, i.e. `wkb::writer::write_geometry`
    /// (little endian) straight into the `BinaryBuilder` (`_w`).
    Wkb,
    /// `geoarrow.wkb` via a hand-written writer for `geo` Point/LineString/Polygon/MultiPolygon
    /// (`_f`, sensitivity; everything else falls back to `wkb::writer`).
    WkbFast,
}

impl Out {
    pub fn suffix(self) -> &'static str {
        match self {
            Out::Union => "n",
            Out::UnionLocal => "l",
            Out::Wkb => "w",
            Out::WkbFast => "f",
        }
    }
    pub fn is_union(self) -> bool {
        matches!(self, Out::Union | Out::UnionLocal)
    }
    pub fn field(self, meta: Arc<Metadata>) -> Field {
        if self.is_union() {
            GeometryType::new(meta)
                .with_coord_type(geoarrow_schema::CoordType::Separated)
                .to_field("", true)
        } else {
            GeoArrowType::Wkb(WkbType::new(meta)).to_field("", true)
        }
    }
}

pub enum GeomOut {
    Union(GeometryBuilder),
    Local(LocalGeometryBuilder),
    Wkb(WkbBuilder<i32>),
    Fast(BinaryBuilder, Arc<Metadata>),
}

impl GeomOut {
    pub fn new(out: Out, return_field: &Field, capacity: usize) -> Result<Self> {
        let meta = GeoArrowType::from_arrow_field(return_field).map_err(ext)?.metadata().clone();
        let t = || GeometryType::new(meta.clone()).with_coord_type(geoarrow_schema::CoordType::Separated);
        Ok(match out {
            Out::Union => GeomOut::Union(GeometryBuilder::new(t())),
            Out::UnionLocal => GeomOut::Local(LocalGeometryBuilder::new(t())),
            Out::Wkb => GeomOut::Wkb(WkbBuilder::new(WkbType::new(meta.clone()))),
            Out::WkbFast => GeomOut::Fast(BinaryBuilder::with_capacity(capacity, 0), meta.clone()),
        })
    }

    #[inline]
    pub fn push(&mut self, g: &geo::Geometry) -> Result<()> {
        match self {
            GeomOut::Fast(b, _) => {
                fast_write(b, g)?;
                b.append_value(b"");
                Ok(())
            }
            _ => self.push_traits(g),
        }
    }

    #[inline]
    pub fn push_traits(&mut self, g: &impl GeometryTrait<T = f64>) -> Result<()> {
        match self {
            GeomOut::Union(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Local(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Wkb(b) => b.push_geometry(Some(g)).map_err(ext),
            GeomOut::Fast(b, _) => {
                let opts = wkb::writer::WriteOptions { endianness: wkb::Endianness::LittleEndian };
                wkb::writer::write_geometry(b, g, &opts).map_err(ext)?;
                b.append_value(b"");
                Ok(())
            }
        }
    }

    /// Bytes that are already WKB (GEOS output). WKB outputs use `GeomOut::Fast` here (a plain
    /// `BinaryBuilder`), so both WKB variants append the GEOS bytes as they are, like E1.
    #[inline]
    pub fn push_wkb_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        match self {
            GeomOut::Fast(b, _) => {
                b.append_value(bytes);
                Ok(())
            }
            GeomOut::Wkb(b) => b.push_wkb(Some(bytes)).map_err(ext),
            _ => {
                let w = wkb::reader::read_wkb(bytes).map_err(ext)?;
                self.push_traits(&w)
            }
        }
    }

    #[inline]
    pub fn push_null(&mut self) {
        match self {
            GeomOut::Union(b) => b.push_null(),
            GeomOut::Local(b) => b.push_geometry(None::<&geo::Geometry>).unwrap(),
            GeomOut::Wkb(b) => b.push_geometry(None::<&geo::Geometry>).unwrap(),
            GeomOut::Fast(b, _) => b.append_null(),
        }
    }

    pub fn finish(self) -> ArrayRef {
        match self {
            GeomOut::Union(b) => b.finish().into_array_ref(),
            GeomOut::Local(b) => b.finish().into_array_ref(),
            GeomOut::Wkb(b) => b.finish().into_array_ref(),
            GeomOut::Fast(mut b, m) => WkbArray::new(b.finish(), m).into_array_ref(),
        }
    }
}

#[inline(always)]
fn put_u32(b: &mut BinaryBuilder, v: u32) {
    b.write_all(&v.to_le_bytes()).unwrap();
}

#[inline(always)]
fn put_coords(b: &mut BinaryBuilder, ls: &geo::LineString) {
    put_u32(b, ls.0.len() as u32);
    for c in &ls.0 {
        b.write_all(&c.x.to_le_bytes()).unwrap();
        b.write_all(&c.y.to_le_bytes()).unwrap();
    }
}

fn put_polygon(b: &mut BinaryBuilder, p: &geo::Polygon) {
    b.write_all(&[1u8]).unwrap();
    put_u32(b, 3);
    let empty = p.exterior().0.is_empty();
    put_u32(b, if empty { 0 } else { 1 + p.interiors().len() as u32 });
    if !empty {
        put_coords(b, p.exterior());
        for r in p.interiors() {
            put_coords(b, r);
        }
    }
}

/// A direct little-endian XY WKB writer for the `geo` types the pipelines produce.
fn fast_write(b: &mut BinaryBuilder, g: &geo::Geometry) -> Result<()> {
    match g {
        geo::Geometry::Point(p) => {
            b.write_all(&[1u8]).unwrap();
            put_u32(b, 1);
            b.write_all(&p.x().to_le_bytes()).unwrap();
            b.write_all(&p.y().to_le_bytes()).unwrap();
        }
        geo::Geometry::LineString(l) => {
            b.write_all(&[1u8]).unwrap();
            put_u32(b, 2);
            put_coords(b, l);
        }
        geo::Geometry::Polygon(p) => put_polygon(b, p),
        geo::Geometry::MultiPolygon(mp) => {
            b.write_all(&[1u8]).unwrap();
            put_u32(b, 6);
            put_u32(b, mp.0.len() as u32);
            for p in &mp.0 {
                put_polygon(b, p);
            }
        }
        other => {
            let opts = wkb::writer::WriteOptions { endianness: wkb::Endianness::LittleEndian };
            wkb::writer::write_geometry(b, other, &opts).map_err(ext)?;
        }
    }
    Ok(())
}

/// GEOS output (G3 §4): GEOS → WKBWriter → bytes. WKB outputs append the bytes; union outputs
/// read them (`read_wkb`) and push to the union builder.
pub struct GeosOut {
    out: GeomOut,
    writer: geos::WKBWriter,
}

impl GeosOut {
    pub fn new(out: Out, return_field: &Field, capacity: usize) -> Result<Self> {
        let mut writer = geos::WKBWriter::new().map_err(ext)?;
        writer.set_output_dimension(geos::CoordDimensions::ThreeD);
        // Both WKB variants append GEOS's bytes unchanged (no re-validation), as E1 and G3 do.
        let out = if out.is_union() { out } else { Out::WkbFast };
        Ok(Self { out: GeomOut::new(out, return_field, capacity)?, writer })
    }
    #[inline]
    pub fn push_geos(&mut self, g: &geos::Geometry) -> Result<()> {
        let bytes = self.writer.write_wkb(g).map_err(ext)?;
        self.out.push_wkb_bytes(&bytes)
    }
    pub fn push_null(&mut self) {
        self.out.push_null()
    }
    pub fn finish(self) -> ArrayRef {
        self.out.finish()
    }
}

pub fn array_of(value: &ColumnarValue, number_rows: usize) -> Result<ArrayRef> {
    Ok(match value {
        ColumnarValue::Array(a) => Arc::clone(a),
        ColumnarValue::Scalar(s) => s.to_array_of_size(number_rows)?,
    })
}

pub fn geo_array(
    value: &ColumnarValue,
    field: &Field,
    number_rows: usize,
) -> Result<Arc<dyn GeoArrowArray>> {
    let a = array_of(value, number_rows)?;
    from_arrow_array(a.as_ref(), field).map_err(ext)
}
