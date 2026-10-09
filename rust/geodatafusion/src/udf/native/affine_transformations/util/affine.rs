//! The 3D affine transformation every affine function is defined by.

use wkt::types::Coord;

/// The transformation `x' = a*x + b*y + c*z + xoff`, `y' = d*x + e*y + f*z + yoff`,
/// `z' = g*x + h*y + i*z + zoff`, with PostGIS's parameter names.
///
/// PostGIS defines ST_Translate, ST_Rotate and friends as ST_Affine with particular
/// parameters, so they go through this too and give the same floating-point results. A 2D
/// coordinate is transformed with Z 0 and stays 2D; M is never changed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Affine3D {
    pub(crate) a: f64,
    pub(crate) b: f64,
    pub(crate) c: f64,
    pub(crate) d: f64,
    pub(crate) e: f64,
    pub(crate) f: f64,
    pub(crate) g: f64,
    pub(crate) h: f64,
    pub(crate) i: f64,
    pub(crate) xoff: f64,
    pub(crate) yoff: f64,
    pub(crate) zoff: f64,
}

impl Affine3D {
    /// A translation by `(dx, dy, dz)`.
    pub(crate) fn translate(dx: f64, dy: f64, dz: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: 0.0,
            g: 0.0,
            h: 0.0,
            i: 1.0,
            xoff: dx,
            yoff: dy,
            zoff: dz,
        }
    }

    /// The transformed coordinate.
    pub(crate) fn apply(&self, coord: Coord<f64>) -> Coord<f64> {
        let (x, y) = (coord.x, coord.y);
        match coord.z {
            Some(z) => Coord {
                x: self.a * x + self.b * y + self.c * z + self.xoff,
                y: self.d * x + self.e * y + self.f * z + self.yoff,
                z: Some(self.g * x + self.h * y + self.i * z + self.zoff),
                m: coord.m,
            },
            None => Coord {
                x: self.a * x + self.b * y + self.xoff,
                y: self.d * x + self.e * y + self.yoff,
                z: None,
                m: coord.m,
            },
        }
    }
}
