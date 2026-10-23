//! The geography forms PostGIS computes in the plane, ST_Buffer and ST_Intersection: the
//! geography is projected to the SRID `_ST_BestSRID` chooses for it, the planar function is
//! applied, and the result is projected back and brought into range, as the cast to geography
//! does. Projecting takes PROJ.

use datafusion::error::Result;
use geo_traits::GeometryTrait;
use wkt::Wkt;

use crate::util::geography::coerce_into_range;
use crate::util::owned::map_coords;

/// The geography, in SRID `srid`, projected to the planar SRID `planar`.
pub(crate) fn project(
    name: &str,
    geom: &impl GeometryTrait<T = f64>,
    srid: i32,
    planar: i32,
) -> Result<Wkt<f64>> {
    transform(name, geom, srid, planar)
}

/// The planar geometry, in SRID `planar`, projected back to geography in SRID `srid`.
pub(crate) fn unproject(
    name: &str,
    geom: &impl GeometryTrait<T = f64>,
    planar: i32,
    srid: i32,
) -> Result<Wkt<f64>> {
    Ok(map_coords(
        &transform(name, geom, planar, srid)?,
        &coerce_into_range,
    ))
}

#[cfg(feature = "proj")]
fn transform(
    name: &str,
    geom: &impl GeometryTrait<T = f64>,
    from: i32,
    to: i32,
) -> Result<Wkt<f64>> {
    use datafusion::common::exec_datafusion_err;

    use crate::udf::proj::srs::transform_geometry;
    use crate::udf::proj::util::crs::proj_definition;
    use crate::udf::proj::util::operation::Definition;
    use crate::util::srid::srid_to_crs;

    let definition = |srid| {
        proj_definition(&srid_to_crs(srid))
            .ok_or_else(|| exec_datafusion_err!("{name}: SRID {srid} has no CRS"))
    };
    let definition = Definition::CrsToCrs {
        from: definition(from)?,
        to: definition(to)?,
    };
    transform_geometry(name, geom, &definition)
}

#[cfg(not(feature = "proj"))]
fn transform(
    name: &str,
    _geom: &impl GeometryTrait<T = f64>,
    _from: i32,
    _to: i32,
) -> Result<Wkt<f64>> {
    datafusion::common::exec_err!("{name}: geography requires the proj feature")
}
