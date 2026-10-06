//! The benchmark UDFs. One struct, parameterised by operation, loop style and output encoding,
//! so every variant shares the same `ScalarUDFImpl` plumbing and differs only in its row loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use arrow_array::builder::{BooleanBuilder, Float64Builder, Int32Builder, StringBuilder};
use arrow_array::{ArrayRef, cast::AsArray};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::error::{DataFusionError, Result};
use datafusion::logical_expr::{
    ColumnarValue, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion::scalar::ScalarValue;
use geo::{PreparedGeometry, Relate as _};
use geo_traits::GeometryTrait;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::{GeoArrowArray, GeoArrowArrayAccessor, downcast_geoarrow_array};
use geoarrow_schema::{
    CoordType, Dimension, GeoArrowType, GeometryType, Metadata, PointType, WkbType,
};

use crate::cg;
use crate::column::{GeoColumn, GeomOut, GeometryColumn, GeosColumn, GeosGeometryBuilder, geo_array};
use crate::kernels::{
    GeoValue, ext, is_empty, npoints, simplify_geometry, to_geo_value, to_geos, x_of,
};

pub static KERNEL_NS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    X,
    NPoints,
    IsEmpty,
    Area,
    Centroid,
    /// Second argument constant (prepared) or a column (plain `Intersects`).
    Intersects,
    Buffer,
    Simplify,
    Translate,
    AsText,
    /// Owned ST_Intersects via DE-9IM `relate` (the algorithm today's st_intersects uses).
    IntersectsRelate,
    /// `geoarrow_expr_geo::intersects` (the `Intersects` trait), array-array only.
    ExprGeoIntersects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    /// `downcast_geoarrow_array!` + a generic function over `GeoArrowArrayAccessor`.
    Typed,
    /// `GeometryColumn` / `GeoColumn` / `GeosColumn` + `for i in 0..number_rows`.
    Unified,
    /// Unified with `fast_to_wkb` (sensitivity variant).
    UnifiedFast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Out {
    /// Point for ST_Centroid, otherwise GeometryType (README "Geometry output type").
    Native,
    Wkb,
    /// The input's type where it's a Polygon (what geoarrow-expr-geo's simplify returns).
    Same,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct BenchUdf {
    name: String,
    op: Op,
    style: Style,
    out: Out,
    signature: Signature,
}

impl BenchUdf {
    pub fn new(name: &str, op: Op, style: Style, out: Out) -> Self {
        Self {
            name: name.to_string(),
            op,
            style,
            out,
            signature: Signature::variadic_any(Volatility::Immutable),
        }
    }
    pub fn udf(name: &str, op: Op, style: Style, out: Out) -> ScalarUDF {
        ScalarUDF::from(Self::new(name, op, style, out))
    }
}

fn geometry_out_field(arg: &Field, out: Out, point: bool) -> Result<Field> {
    let geo_type = GeoArrowType::from_arrow_field(arg).map_err(ext)?;
    let meta: Arc<Metadata> = geo_type.metadata().clone();
    Ok(match out {
        Out::Wkb => GeoArrowType::Wkb(WkbType::new(meta)).to_field("", true),
        Out::Native if point => PointType::new(Dimension::XY, meta)
            .with_coord_type(CoordType::Separated)
            .to_field("", true),
        Out::Native => GeometryType::new(meta)
            .with_coord_type(CoordType::Separated)
            .to_field("", true),
        Out::Same => match geo_type {
            GeoArrowType::Polygon(t) => t.with_dimension(Dimension::XY).to_field("", true),
            _ => GeometryType::new(meta).to_field("", true),
        },
    })
}

fn scalar_f64(v: &ColumnarValue) -> Result<f64> {
    match v {
        ColumnarValue::Scalar(ScalarValue::Float64(Some(x))) => Ok(*x),
        ColumnarValue::Scalar(ScalarValue::Int64(Some(x))) => Ok(*x as f64),
        other => Err(DataFusionError::Plan(format!("expected a float constant, got {other:?}"))),
    }
}

impl ScalarUDFImpl for BenchUdf {
    fn name(&self) -> &str {
        &self.name
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("use return_field_from_args".into()))
    }
    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        let arg = args.arg_fields[0].as_ref();
        let f = match self.op {
            Op::X | Op::Area => Field::new("", DataType::Float64, true),
            Op::NPoints => Field::new("", DataType::Int32, true),
            Op::IsEmpty | Op::Intersects | Op::IntersectsRelate | Op::ExprGeoIntersects => {
                Field::new("", DataType::Boolean, true)
            }
            Op::AsText => Field::new("", DataType::Utf8, true),
            Op::Centroid => geometry_out_field(arg, self.out, true)?,
            Op::Buffer | Op::Simplify | Op::Translate => geometry_out_field(arg, self.out, false)?,
        };
        Ok(Arc::new(f.with_name(self.name.clone())))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        cg::start(2);
        let t0 = Instant::now();
        let result = if self.op == Op::ExprGeoIntersects {
            expr_geo_intersects(&args)
        } else {
            match self.style {
            Style::Typed => typed(self.op, &args),
            Style::Unified => unified(self.op, &args),
            Style::UnifiedFast => {
                crate::column::FAST_WKB.store(true, Ordering::Relaxed);
                let r = unified(self.op, &args);
                crate::column::FAST_WKB.store(false, Ordering::Relaxed);
                r
            }
        }
        };
        KERNEL_NS.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        cg::stop(2);
        Ok(ColumnarValue::Array(result?))
    }
}

// ---------------------------------------------------------------------------------------------
// Typed style.

fn typed(op: Op, args: &ScalarFunctionArgs) -> Result<ArrayRef> {
    let n = args.number_rows;
    let arr = geo_array(&args.args[0], &args.arg_fields[0], n)?;
    let arr = arr.as_ref();
    match op {
        Op::X => downcast_geoarrow_array!(arr, t_x),
        Op::NPoints => downcast_geoarrow_array!(arr, t_npoints),
        Op::IsEmpty => downcast_geoarrow_array!(arr, t_is_empty),
        Op::Area => downcast_geoarrow_array!(arr, t_area),
        Op::Centroid => {
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_centroid, &mut out)?;
            Ok(out.finish())
        }
        Op::Intersects => match &args.args[1] {
            ColumnarValue::Scalar(_) => {
                // A constant converted to `geo` once and prepared, as in today's st_intersects.
                let other = geo_array(&args.args[1], &args.arg_fields[1], 1)?;
                let other = other.as_ref();
                match downcast_geoarrow_array!(other, t_first_geo)? {
                    GeoValue::Geometry(g) => {
                        let prepared = PreparedGeometry::from(g);
                        downcast_geoarrow_array!(arr, t_intersects_prepared, &prepared)
                    }
                    _ => Err(DataFusionError::NotImplemented("NULL/EMPTY constant".into())),
                }
            }
            ColumnarValue::Array(_) => Err(DataFusionError::NotImplemented(
                "typed array-array intersects is not part of E1".into(),
            )),
        },
        Op::Buffer => {
            let r = scalar_f64(&args.args[1])?;
            let mut out = GeosGeometryBuilder::try_new(&args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_buffer, r, &mut out)?;
            Ok(out.finish())
        }
        Op::Simplify => {
            let t = scalar_f64(&args.args[1])?;
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_simplify, t, &mut out)?;
            Ok(out.finish())
        }
        Op::Translate => {
            let dx = scalar_f64(&args.args[1])?;
            let dy = scalar_f64(&args.args[2])?;
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_translate, dx, dy, &mut out)?;
            Ok(out.finish())
        }
        Op::AsText => downcast_geoarrow_array!(arr, t_astext),
        Op::IntersectsRelate | Op::ExprGeoIntersects => {
            Err(DataFusionError::NotImplemented("not part of E1".into()))
        }
    }
}

fn t_x<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<ArrayRef> {
    let mut b = Float64Builder::with_capacity(a.len());
    for item in a.iter() {
        match item {
            Some(g) => b.append_option(x_of(&g.map_err(ext)?)?),
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

fn t_npoints<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<ArrayRef> {
    let mut b = Int32Builder::with_capacity(a.len());
    for item in a.iter() {
        match item {
            Some(g) => b.append_value(npoints(&g.map_err(ext)?)),
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

fn t_is_empty<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<ArrayRef> {
    let mut b = BooleanBuilder::with_capacity(a.len());
    for item in a.iter() {
        match item {
            Some(g) => b.append_value(is_empty(&g.map_err(ext)?)),
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

fn t_area<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<ArrayRef> {
    use geo::Area as _;
    let mut b = Float64Builder::with_capacity(a.len());
    for item in a.iter() {
        match item {
            Some(g) => match to_geo_value(&g.map_err(ext)?)? {
                GeoValue::Null => b.append_null(),
                GeoValue::Empty => b.append_value(0.0),
                GeoValue::Geometry(g) => b.append_value(g.unsigned_area()),
            },
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

fn t_centroid<'a>(a: &'a impl GeoArrowArrayAccessor<'a>, out: &mut GeomOut) -> Result<()> {
    use geo::Centroid as _;
    for item in a.iter() {
        match item {
            Some(g) => match to_geo_value(&g.map_err(ext)?)? {
                GeoValue::Null => out.push_null(),
                GeoValue::Empty => out.push_empty(),
                GeoValue::Geometry(g) => match g.centroid() {
                    Some(c) => out.push(&c)?,
                    None => out.push_empty(),
                },
            },
            None => out.push_null(),
        }
    }
    Ok(())
}

fn t_first_geo<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<GeoValue> {
    match a.iter().next().flatten() {
        Some(g) => to_geo_value(&g.map_err(ext)?),
        None => Ok(GeoValue::Null),
    }
}

fn t_intersects_prepared<'a>(
    a: &'a impl GeoArrowArrayAccessor<'a>,
    prepared: &PreparedGeometry<'static, geo::Geometry>,
) -> Result<ArrayRef> {
    let mut b = BooleanBuilder::with_capacity(a.len());
    for item in a.iter() {
        match item {
            Some(g) => match to_geo_value(&g.map_err(ext)?)? {
                GeoValue::Null => b.append_null(),
                GeoValue::Empty => b.append_value(false),
                GeoValue::Geometry(g) => b.append_value(g.relate(prepared).is_intersects()),
            },
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

fn t_buffer<'a>(
    a: &'a impl GeoArrowArrayAccessor<'a>,
    r: f64,
    out: &mut GeosGeometryBuilder,
) -> Result<()> {
    use geos::Geom as _;
    for item in a.iter() {
        match item {
            Some(g) => {
                let g = to_geos(&g.map_err(ext)?)?;
                out.push_geos(&g.buffer(r, 8).map_err(ext)?)?
            }
            None => out.push_null(),
        }
    }
    Ok(())
}

fn t_simplify<'a>(a: &'a impl GeoArrowArrayAccessor<'a>, t: f64, out: &mut GeomOut) -> Result<()> {
    for item in a.iter() {
        match item {
            Some(g) => match to_geo_value(&g.map_err(ext)?)? {
                GeoValue::Null => out.push_null(),
                GeoValue::Empty => out.push_empty(),
                GeoValue::Geometry(g) => out.push(&simplify_geometry(&g, t))?,
            },
            None => out.push_null(),
        }
    }
    Ok(())
}

fn t_translate<'a>(
    a: &'a impl GeoArrowArrayAccessor<'a>,
    dx: f64,
    dy: f64,
    out: &mut GeomOut,
) -> Result<()> {
    use geo::Translate as _;
    for item in a.iter() {
        match item {
            Some(g) => match to_geo_value(&g.map_err(ext)?)? {
                GeoValue::Null => out.push_null(),
                GeoValue::Empty => out.push_empty(),
                GeoValue::Geometry(g) => out.push(&g.translate(dx, dy))?,
            },
            None => out.push_null(),
        }
    }
    Ok(())
}

fn write_wkt(b: &mut StringBuilder, g: &impl GeometryTrait<T = f64>) -> Result<()> {
    wkt::to_wkt::write_geometry(b, g).map_err(ext)?;
    b.append_value("");
    Ok(())
}

fn t_astext<'a>(a: &'a impl GeoArrowArrayAccessor<'a>) -> Result<ArrayRef> {
    let mut b = StringBuilder::with_capacity(a.len(), 0);
    for item in a.iter() {
        match item {
            Some(g) => write_wkt(&mut b, &g.map_err(ext)?)?,
            None => b.append_null(),
        }
    }
    Ok(Arc::new(b.finish()))
}

// ---------------------------------------------------------------------------------------------
// Unified style.

fn unified(op: Op, args: &ScalarFunctionArgs) -> Result<ArrayRef> {
    let n = args.number_rows;
    match op {
        Op::X => {
            let geom = GeometryColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut b = Float64Builder::with_capacity(n);
            for i in 0..n {
                match geom.get(i)? {
                    Some(g) => b.append_option(x_of(&g)?),
                    None => b.append_null(),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::NPoints => {
            let geom = GeometryColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut b = Int32Builder::with_capacity(n);
            for i in 0..n {
                match geom.get(i)? {
                    Some(g) => b.append_value(npoints(&g)),
                    None => b.append_null(),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::IsEmpty => {
            let geom = GeometryColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut b = BooleanBuilder::with_capacity(n);
            for i in 0..n {
                match geom.get(i)? {
                    Some(g) => b.append_value(is_empty(&g)),
                    None => b.append_null(),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::Area => {
            use geo::Area as _;
            let geom = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut b = Float64Builder::with_capacity(n);
            for i in 0..n {
                match geom.get(i)?.as_ref() {
                    GeoValue::Null => b.append_null(),
                    GeoValue::Empty => b.append_value(0.0),
                    GeoValue::Geometry(g) => b.append_value(g.unsigned_area()),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::Centroid => {
            use geo::Centroid as _;
            let geom = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            for i in 0..n {
                match geom.get(i)?.as_ref() {
                    GeoValue::Null => out.push_null(),
                    GeoValue::Empty => out.push_empty(),
                    GeoValue::Geometry(g) => match g.centroid() {
                        Some(c) => out.push(&c)?,
                        None => out.push_empty(),
                    },
                }
            }
            Ok(out.finish())
        }
        Op::Intersects => {
            let a = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let b_col = GeoColumn::try_new(&args.args[1], &args.arg_fields[1])?;
            let mut b = BooleanBuilder::with_capacity(n);
            if let Some(GeoValue::Geometry(q)) = b_col.as_scalar() {
                // Constant: prepared once (G2's template).
                let prepared = PreparedGeometry::from(q.clone());
                for i in 0..n {
                    match a.get(i)?.as_ref() {
                        GeoValue::Null => b.append_null(),
                        GeoValue::Empty => b.append_value(false),
                        GeoValue::Geometry(g) => b.append_value(g.relate(&prepared).is_intersects()),
                    }
                }
            } else {
                use geo::Intersects as _;
                for i in 0..n {
                    match (a.get(i)?.as_ref(), b_col.get(i)?.as_ref()) {
                        (GeoValue::Null, _) | (_, GeoValue::Null) => b.append_null(),
                        (GeoValue::Empty, _) | (_, GeoValue::Empty) => b.append_value(false),
                        (GeoValue::Geometry(x), GeoValue::Geometry(y)) => {
                            b.append_value(x.intersects(y))
                        }
                    }
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::IntersectsRelate => {
            let a = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let b_col = GeoColumn::try_new(&args.args[1], &args.arg_fields[1])?;
            let mut b = BooleanBuilder::with_capacity(n);
            for i in 0..n {
                match (a.get(i)?.as_ref(), b_col.get(i)?.as_ref()) {
                    (GeoValue::Null, _) | (_, GeoValue::Null) => b.append_null(),
                    (GeoValue::Empty, _) | (_, GeoValue::Empty) => b.append_value(false),
                    (GeoValue::Geometry(x), GeoValue::Geometry(y)) => {
                        b.append_value(x.relate(y).is_intersects())
                    }
                }
            }
            Ok(Arc::new(b.finish()))
        }
        Op::ExprGeoIntersects => unreachable!(),
        Op::Buffer => {
            use geos::Geom as _;
            let geom = GeosColumn::try_new(&args.args[0], &args.arg_fields[0], n)?;
            let r = scalar_f64(&args.args[1])?;
            let mut out = GeosGeometryBuilder::try_new(&args.return_field, n)?;
            for i in 0..n {
                match geom.get(i) {
                    Some(g) => out.push_geos(&g.buffer(r, 8).map_err(ext)?)?,
                    None => out.push_null(),
                }
            }
            Ok(out.finish())
        }
        Op::Simplify => {
            let geom = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let t = scalar_f64(&args.args[1])?;
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            for i in 0..n {
                match geom.get(i)?.as_ref() {
                    GeoValue::Null => out.push_null(),
                    GeoValue::Empty => out.push_empty(),
                    GeoValue::Geometry(g) => out.push(&simplify_geometry(g, t))?,
                }
            }
            Ok(out.finish())
        }
        Op::Translate => {
            use geo::Translate as _;
            let geom = GeoColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let dx = scalar_f64(&args.args[1])?;
            let dy = scalar_f64(&args.args[2])?;
            let mut out = GeomOut::try_new(&args.return_field, n)?;
            for i in 0..n {
                match geom.get(i)?.as_ref() {
                    GeoValue::Null => out.push_null(),
                    GeoValue::Empty => out.push_empty(),
                    GeoValue::Geometry(g) => out.push(&g.translate(dx, dy))?,
                }
            }
            Ok(out.finish())
        }
        Op::AsText => {
            let geom = GeometryColumn::try_new(&args.args[0], &args.arg_fields[0])?;
            let mut b = StringBuilder::with_capacity(n, 0);
            for i in 0..n {
                match geom.get(i)? {
                    Some(g) => write_wkt(&mut b, &g)?,
                    None => b.append_null(),
                }
            }
            Ok(Arc::new(b.finish()))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Helper: a WKT constant as a GeoArrow WKB scalar, so constant folding yields a Binary literal
// with the extension metadata.

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct WkbConst {
    signature: Signature,
}

impl WkbConst {
    pub fn udf() -> ScalarUDF {
        ScalarUDF::from(Self {
            signature: Signature::exact(vec![DataType::Utf8], Volatility::Immutable),
        })
    }
}

impl ScalarUDFImpl for WkbConst {
    fn name(&self) -> &str {
        "e1_wkb"
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Err(DataFusionError::Internal("use return_field_from_args".into()))
    }
    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(Arc::new(GeoArrowType::Wkb(WkbType::new(Default::default())).to_field("e1_wkb", true)))
    }
    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let arr = crate::column::array_of(&args.args[0], args.number_rows)?;
        let mut b = WkbBuilder::<i32>::new(WkbType::new(Default::default()));
        for s in arr.as_string::<i32>().iter() {
            let g: Option<wkt::Wkt<f64>> = s.map(|s| s.parse().unwrap());
            b.push_geometry(g.as_ref()).map_err(ext)?;
        }
        let out = b.finish().into_array_ref();
        Ok(match &args.args[0] {
            ColumnarValue::Scalar(_) => ColumnarValue::Scalar(ScalarValue::try_from_array(&out, 0)?),
            ColumnarValue::Array(_) => ColumnarValue::Array(out),
        })
    }
}

fn expr_geo_intersects(args: &ScalarFunctionArgs) -> Result<ArrayRef> {
    let n = args.number_rows;
    let a = geo_array(&args.args[0], &args.arg_fields[0], n)?;
    let b = geo_array(&args.args[1], &args.arg_fields[1], n)?;
    Ok(Arc::new(geoarrow_expr_geo::intersects(a.as_ref(), b.as_ref()).map_err(ext)?))
}
