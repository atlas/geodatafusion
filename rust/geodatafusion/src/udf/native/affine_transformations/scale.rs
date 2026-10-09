use std::sync::LazyLock;

use arrow_array::{Array, Float64Array};
use arrow_schema::{DataType, FieldRef};
use datafusion::common::{exec_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    Volatility,
};
use datafusion_macros::user_doc;
use geo_traits::GeometryTrait;
use wkt::Wkt;
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::args::optional_float_arg;
use crate::util::field::{geometry_array, input_metadata, wkb_return_field};
use crate::util::kernel::{GeometryKernel, map_geometry_to_wkb};
use crate::util::owned::{OwnedColumn, map_coords};
use crate::util::signature::{Arg, coerce_args};

/// PostGIS:
/// - ST_Scale(geometry geomA, float XFactor, float YFactor)
/// - ST_Scale(geometry geomA, float XFactor, float YFactor, float ZFactor)
/// - ST_Scale(geometry geom, geometry factor)
/// - ST_Scale(geometry geom, geometry factor, geometry origin)
///
/// PostGIS only names `origin`.
static ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Geometry, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Geometry, Arg::Geometry],
    &[Arg::Geometry, Arg::Geometry, Arg::Geometry],
];

static SIGNATURE: LazyLock<Signature> =
    LazyLock::new(|| Signature::user_defined(Volatility::Immutable));

/// Scales a geometry by given factors.
#[user_doc(
    doc_section(label = "Affine Transformations"),
    description = "Scales a geometry by multiplying its ordinates by the factors: XFactor, YFactor and ZFactor (default 1), or the ordinates of a factor POINT, which also scales M if it has one. A factor POINT without Z or M leaves them unchanged. With an origin POINT, the geometry is scaled about it rather than about (0, 0); M is always scaled about 0. Unlike most functions, the factor and origin SRIDs aren't checked, as in PostGIS.",
    syntax_example = "ST_Scale(geomA, XFactor, YFactor, ZFactor)",
    alternative_syntax = "ST_Scale(geom, factor, origin)",
    argument(name = "geomA", description = "geometry"),
    argument(name = "XFactor", description = "float8, or factor: a POINT geometry"),
    argument(name = "YFactor", description = "float8, or origin: a POINT geometry"),
    argument(name = "ZFactor", description = "float8, default 1"),
    related_udf(name = "st_affine"),
    related_udf(name = "st_transscale")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Scale;

impl Scale {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Scale {
    fn name(&self) -> &str {
        "st_scale"
    }

    fn signature(&self) -> &Signature {
        &SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(
            self.name(),
            input_metadata(&args.arg_fields[0]),
        ))
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(scale_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn scale_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    // Coercion makes float factors Float64 and factor points geometries.
    let result = if args.arg_fields[1].data_type().is_numeric() {
        let kernel = ScaleKernel {
            xfactor: optional_float_arg(&args, 1, 1.0)?,
            yfactor: optional_float_arg(&args, 2, 1.0)?,
            zfactor: optional_float_arg(&args, 3, 1.0)?,
        };
        map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?
    } else {
        let origin = match args.args.get(2) {
            Some(origin) => Some(OwnedColumn::try_new(
                origin,
                &args.arg_fields[2],
                args.number_rows,
            )?),
            None => None,
        };
        let kernel = ScaleByPointKernel {
            factor: OwnedColumn::try_new(&args.args[1], &args.arg_fields[1], args.number_rows)?,
            origin,
        };
        map_geometry_to_wkb(geometries.as_ref(), &kernel, &args.return_field)?
    };
    Ok(ColumnarValue::Array(result.to_array_ref()))
}

struct ScaleKernel {
    xfactor: Float64Array,
    yfactor: Float64Array,
    zfactor: Float64Array,
}

impl GeometryKernel for ScaleKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        // SQL NULL in any argument, SQL NULL out.
        if self.xfactor.is_null(row) || self.yfactor.is_null(row) || self.zfactor.is_null(row) {
            return Ok(None);
        }
        // PostGIS defines this form as scaling by ST_MakePoint(XFactor, YFactor, ZFactor).
        let factor = Coord {
            x: self.xfactor.value(row),
            y: self.yfactor.value(row),
            z: Some(self.zfactor.value(row)),
            m: None,
        };
        Ok(Some(map_coords(geom, &|c| scale_about(c, &factor, None))))
    }
}

struct ScaleByPointKernel {
    factor: OwnedColumn,
    origin: Option<OwnedColumn>,
}

impl GeometryKernel for ScaleByPointKernel {
    type Output = Wkt<f64>;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        row: usize,
    ) -> GeoDataFusionResult<Option<Wkt<f64>>> {
        let Some(factor) = self.factor.get(row) else {
            return Ok(None);
        };
        let factor = point_coord(factor)?;
        let origin = match &self.origin {
            None => None,
            Some(origin) => match origin.get(row) {
                None => return Ok(None),
                // An empty origin is (0, 0), as in PostGIS.
                Some(origin) => point_coord(origin)?,
            },
        };
        let Some(factor) = factor else {
            return Err(exec_datafusion_err!(
                "st_scale: Scale factor geometry parameter must not be empty"
            )
            .into());
        };
        Ok(Some(map_coords(geom, &|c| {
            scale_about(c, &factor, origin.as_ref())
        })))
    }
}

/// The coordinate of a factor or origin POINT; `None` if it is empty.
fn point_coord(geom: &Wkt<f64>) -> GeoDataFusionResult<Option<Coord<f64>>> {
    let Wkt::Point(point) = geom else {
        return Err(exec_datafusion_err!(
            "st_scale: Scale factor geometry parameter must be a point"
        )
        .into());
    };
    Ok(point.coord().cloned())
}

/// Scales each ordinate the factor has, about the origin's ordinate (0 if it has none). Like
/// PostGIS, the origin is subtracted, the factor applied and the origin added back, and M is
/// scaled about 0.
fn scale_about(coord: Coord<f64>, factor: &Coord<f64>, origin: Option<&Coord<f64>>) -> Coord<f64> {
    let about = |value: f64, factor: f64, origin: Option<f64>| match origin {
        Some(origin) => (value - origin) * factor + origin,
        None => value * factor,
    };
    Coord {
        x: about(coord.x, factor.x, origin.map(|o| o.x)),
        y: about(coord.y, factor.y, origin.map(|o| o.y)),
        z: coord.z.map(|z| match factor.z {
            Some(zf) => about(z, zf, origin.map(|o| o.z.unwrap_or(0.0))),
            None => z,
        }),
        m: coord.m.map(|m| factor.m.map_or(m, |mf| m * mf)),
    }
}
