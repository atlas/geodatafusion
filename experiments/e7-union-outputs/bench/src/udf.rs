//! Benchmark UDFs (typed style only: `downcast_geoarrow_array!`, the D1 decision). Producers are
//! parameterised by output encoding; consumers read whatever arrives through the same downcast.
//! Kernels are E1's typed kernels (`experiments/e1-performance/src/udf.rs`), unchanged apart
//! from the output builder.

use std::sync::Arc;

use arrow_array::builder::{BooleanBuilder, Float64Builder, StringBuilder};
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
use geoarrow_schema::{GeoArrowType, WkbType};

use crate::cg;
use crate::kernels::{GeoValue, ext, simplify_geometry, to_geo_value, to_geos, x_of};
use crate::output::{GeomOut, GeosOut, Out, array_of, geo_array};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    X,
    Area,
    AsText,
    /// Second argument constant (prepared).
    Intersects,
    Centroid,
    Simplify,
    Translate,
    Buffer,
}

impl Op {
    fn is_producer(self) -> bool {
        matches!(self, Op::Centroid | Op::Simplify | Op::Translate | Op::Buffer)
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct BenchUdf {
    name: String,
    op: Op,
    out: Out,
    signature: Signature,
}

impl BenchUdf {
    pub fn udf(name: &str, op: Op, out: Out) -> ScalarUDF {
        ScalarUDF::from(Self {
            name: name.to_string(),
            op,
            out,
            signature: Signature::variadic_any(Volatility::Immutable),
        })
    }
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
            Op::Intersects => Field::new("", DataType::Boolean, true),
            Op::AsText => Field::new("", DataType::Utf8, true),
            _ => {
                let meta = GeoArrowType::from_arrow_field(arg).map_err(ext)?.metadata().clone();
                self.out.field(meta)
            }
        };
        Ok(Arc::new(f.with_name(self.name.clone())))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        cg::start(2);
        let r = typed(self.op, self.out, &args);
        cg::stop(2);
        Ok(ColumnarValue::Array(r?))
    }
}

fn typed(op: Op, out: Out, args: &ScalarFunctionArgs) -> Result<ArrayRef> {
    let n = args.number_rows;
    let arr = geo_array(&args.args[0], &args.arg_fields[0], n)?;
    let arr = arr.as_ref();
    debug_assert!(op.is_producer() || out == Out::Wkb);
    match op {
        Op::X => downcast_geoarrow_array!(arr, t_x),
        Op::Area => downcast_geoarrow_array!(arr, t_area),
        Op::AsText => downcast_geoarrow_array!(arr, t_astext),
        Op::Intersects => {
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
        Op::Centroid => {
            let mut b = GeomOut::new(out, &args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_centroid, &mut b)?;
            Ok(b.finish())
        }
        Op::Simplify => {
            let t = scalar_f64(&args.args[1])?;
            let mut b = GeomOut::new(out, &args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_simplify, t, &mut b)?;
            Ok(b.finish())
        }
        Op::Translate => {
            let dx = scalar_f64(&args.args[1])?;
            let dy = scalar_f64(&args.args[2])?;
            let mut b = GeomOut::new(out, &args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_translate, dx, dy, &mut b)?;
            Ok(b.finish())
        }
        Op::Buffer => {
            let r = scalar_f64(&args.args[1])?;
            let mut b = GeosOut::new(out, &args.return_field, n)?;
            downcast_geoarrow_array!(arr, t_buffer, r, &mut b)?;
            Ok(b.finish())
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
                GeoValue::Geometry(g) => match g.centroid() {
                    Some(c) => out.push(&geo::Geometry::Point(c))?,
                    None => out.push_null(),
                },
                _ => out.push_null(),
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

fn t_buffer<'a>(a: &'a impl GeoArrowArrayAccessor<'a>, r: f64, out: &mut GeosOut) -> Result<()> {
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
                GeoValue::Geometry(g) => out.push(&simplify_geometry(&g, t))?,
                _ => out.push_null(),
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
                GeoValue::Geometry(g) => out.push(&g.translate(dx, dy))?,
                _ => out.push_null(),
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

// A WKT constant as a GeoArrow WKB scalar (E1's `e1_wkb`), so constant folding yields a Binary
// literal with the extension metadata.
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
        let arr = array_of(&args.args[0], args.number_rows)?;
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

pub fn register(ctx: &datafusion::prelude::SessionContext) {
    ctx.register_udf(WkbConst::udf());
    for (name, op) in [("x", Op::X), ("area", Op::Area), ("astext", Op::AsText), ("intersects", Op::Intersects)] {
        ctx.register_udf(BenchUdf::udf(name, op, Out::Wkb));
    }
    for (name, op) in [
        ("centroid", Op::Centroid),
        ("simplify", Op::Simplify),
        ("translate", Op::Translate),
        ("buffer", Op::Buffer),
    ] {
        for out in [Out::Union, Out::UnionLocal, Out::Wkb, Out::WkbFast] {
            ctx.register_udf(BenchUdf::udf(&format!("{name}_{}", out.suffix()), op, out));
        }
    }
}
