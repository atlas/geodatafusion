//! Round-trip a WKT literal through geoarrow-array's GeometryBuilder (native) and WkbBuilder.
//! Shared between the geoarrow 0.8 crate (h2c bin) and the geoarrow 0.9 crate (via #[path]).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use geoarrow_array::GeoArrowArrayAccessor;
use geoarrow_array::builder::{GeometryBuilder, WkbBuilder};
use geoarrow_schema::{GeometryType, WkbType};

pub enum Rt {
    Same,
    Changed(String),
    Error(String),
    Panic(String),
}

impl Rt {
    pub fn label(&self) -> String {
        match self {
            Rt::Same => "same".into(),
            Rt::Changed(s) => format!("CHANGED -> {s}"),
            Rt::Error(s) => format!("ERROR {s}"),
            Rt::Panic(s) => format!("PANIC {s}"),
        }
    }
}

pub fn strip_srid(s: &str) -> &str {
    if s.len() > 5 && s[..5].eq_ignore_ascii_case("SRID=") {
        if let Some(i) = s.find(';') {
            return &s[i + 1..];
        }
    }
    s
}

pub fn to_wkt(g: &impl geo_traits::GeometryTrait<T = f64>) -> String {
    match catch_unwind(AssertUnwindSafe(|| {
        let mut s = String::new();
        wkt::to_wkt::write_geometry(&mut s, g).map(|_| s).unwrap_or_else(|e| format!("<wkt write error {e}>"))
    })) {
        Ok(s) => s,
        Err(p) => format!("<wkt writer PANIC: {}>", panic_msg(p)),
    }
}

fn panic_msg(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

pub fn native(g: &wkt::Wkt<f64>) -> Rt {
    let expected = to_wkt(g);
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut b = GeometryBuilder::new(GeometryType::new(Arc::new(Default::default())));
        b.push_geometry(Some(g)).map_err(|e| e.to_string())?;
        let arr = b.finish();
        let v = arr.value(0).map_err(|e| e.to_string())?;
        Ok::<String, String>(to_wkt(&v))
    }));
    match r {
        Ok(Ok(s)) if s == expected => Rt::Same,
        Ok(Ok(s)) => Rt::Changed(s),
        Ok(Err(e)) => Rt::Error(e),
        Err(p) => Rt::Panic(panic_msg(p)),
    }
}

pub fn wkb_rt(g: &wkt::Wkt<f64>) -> Rt {
    let expected = to_wkt(g);
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut b = WkbBuilder::<i32>::new(WkbType::new(Arc::new(Default::default())));
        b.push_geometry(Some(g)).map_err(|e| e.to_string())?;
        let arr = b.finish();
        let v = arr.value(0).map_err(|e| e.to_string())?;
        Ok::<String, String>(to_wkt(&v))
    }));
    match r {
        Ok(Ok(s)) if s == expected => Rt::Same,
        Ok(Ok(s)) => Rt::Changed(s),
        Ok(Err(e)) => Rt::Error(e),
        Err(p) => Rt::Panic(panic_msg(p)),
    }
}

/// Native round trip of raw WKB bytes (for inputs WKT can't express, like mixed dimensions).
pub fn native_from_wkb(bytes: &[u8]) -> Rt {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let g = wkb::reader::read_wkb(bytes).map_err(|e| e.to_string())?;
        let mut b = GeometryBuilder::new(GeometryType::new(Arc::new(Default::default())));
        b.push_geometry(Some(&g)).map_err(|e| e.to_string())?;
        let arr = b.finish();
        let v = arr.value(0).map_err(|e| e.to_string())?;
        Ok::<String, String>(to_wkt(&v))
    }));
    match r {
        Ok(Ok(s)) => Rt::Changed(s),
        Ok(Err(e)) => Rt::Error(e),
        Err(p) => Rt::Panic(panic_msg(p)),
    }
}

/// WKB for GEOMETRYCOLLECTION(POINT Z (1 2 3), POINT(4 5)): an XY collection holding a Z point
/// and an XY point (mixed dimensions; PostGIS rejects this).
pub fn mixed_dim_gc_wkb() -> Vec<u8> {
    let mut v = vec![1u8];
    v.extend(7u32.to_le_bytes());
    v.extend(2u32.to_le_bytes());
    v.push(1);
    v.extend(1001u32.to_le_bytes());
    for c in [1.0f64, 2.0, 3.0] {
        v.extend(c.to_le_bytes());
    }
    v.push(1);
    v.extend(1u32.to_le_bytes());
    for c in [4.0f64, 5.0] {
        v.extend(c.to_le_bytes());
    }
    v
}

/// WKB for GEOMETRYCOLLECTION Z (POINT Z (1 2 3), POINT (4 5)): a Z collection holding an XY point.
pub fn mixed_dim_gcz_wkb() -> Vec<u8> {
    let mut v = vec![1u8];
    v.extend(1007u32.to_le_bytes());
    v.extend(2u32.to_le_bytes());
    v.push(1);
    v.extend(1001u32.to_le_bytes());
    for c in [1.0f64, 2.0, 3.0] {
        v.extend(c.to_le_bytes());
    }
    v.push(1);
    v.extend(1u32.to_le_bytes());
    for c in [4.0f64, 5.0] {
        v.extend(c.to_le_bytes());
    }
    v
}

pub fn minimal_repros() {
    let cases = [
        "GEOMETRYCOLLECTION(POINT(1 2))",
        "GEOMETRYCOLLECTION(LINESTRING(0 0,1 1))",
        "GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(1 2)))",
        "GEOMETRYCOLLECTION(POINT(1 2),POINT(3 4))",
        "GEOMETRYCOLLECTION Z(POINT Z(1 2 3))",
        "GEOMETRYCOLLECTION EMPTY",
        "MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))",
        "MULTIPOINT(EMPTY,(1 2))",
        "POINT EMPTY",
    ];
    println!("| input | GeometryBuilder (native) | WkbBuilder |");
    println!("|---|---|---|");
    for c in cases {
        match catch_unwind(|| c.parse::<wkt::Wkt<f64>>()) {
            Ok(Ok(g)) => println!("| `{c}` | {} | {} |", native(&g).label(), wkb_rt(&g).label()),
            Ok(Err(e)) => println!("| `{c}` | wkt parse error: {e} | |"),
            Err(p) => println!("| `{c}` | wkt parse PANIC: {} | |", panic_msg(p)),
        }
    }
    println!("| WKB `GEOMETRYCOLLECTION(POINT Z(1 2 3), POINT(4 5))` (XY GC, Z member) | {} | n/a |", native_from_wkb(&mixed_dim_gc_wkb()).label());
    println!("| WKB `GEOMETRYCOLLECTION Z(POINT Z(1 2 3), POINT(4 5))` (Z GC, XY member) | {} | n/a |", native_from_wkb(&mixed_dim_gcz_wkb()).label());
}
