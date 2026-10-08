use std::sync::Arc;

use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::plan_err;
use datafusion::error::Result;
use datafusion::logical_expr::planner::TypePlanner;
use datafusion::sql::sqlparser::ast::{self, ObjectNamePart};
use geoarrow_schema::{BoxType, Dimension, Metadata, WkbType};

use crate::util::srid::{SRID_UNKNOWN, clamp_srid, srid_to_crs};

/// The geometry types a `geometry(type, srid)` type modifier may name, as PostGIS spells them
/// without a Z, M or ZM suffix.
const GEOMETRY_TYPES: &[&str] = &[
    "geometry",
    "point",
    "linestring",
    "polygon",
    "multipoint",
    "multilinestring",
    "multipolygon",
    "geometrycollection",
    "circularstring",
    "compoundcurve",
    "curvepolygon",
    "multicurve",
    "multisurface",
    "polyhedralsurface",
    "triangle",
    "tin",
];

/// Plans PostGIS's `geometry`, `box2d` and `box3d` SQL types.
///
/// | SQL | Field |
/// |---|---|
/// | `geometry`, `geometry(type)` | `Binary`, `geoarrow.wkb`, no CRS |
/// | `geometry(type, srid)` | as above, with the SRID's CRS |
/// | `box2d`, `box3d` | `geoarrow.box`, XY or XYZ |
///
/// A session has one type planner, so this one hands any other type to an optional fallback.
/// Install it when building the session; [`register`](crate::register) adds the casts that
/// convert values to these types:
///
/// ```
/// use std::sync::Arc;
///
/// use datafusion::execution::SessionStateBuilder;
/// use datafusion::prelude::SessionContext;
/// use geodatafusion::sql::GeoTypePlanner;
///
/// let state = SessionStateBuilder::new()
///     .with_default_features()
///     .with_type_planner(Arc::new(GeoTypePlanner::new()))
///     .build();
/// let ctx = SessionContext::new_with_state(state);
/// geodatafusion::register(&ctx);
/// ```
#[derive(Debug, Default)]
pub struct GeoTypePlanner {
    fallback: Option<Arc<dyn TypePlanner>>,
}

impl GeoTypePlanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// A planner that hands the types it doesn't plan to `fallback`.
    pub fn with_fallback(fallback: Arc<dyn TypePlanner>) -> Self {
        Self {
            fallback: Some(fallback),
        }
    }
}

impl TypePlanner for GeoTypePlanner {
    fn plan_type_field(&self, sql_type: &ast::DataType) -> Result<Option<FieldRef>> {
        if let ast::DataType::Custom(name, modifiers) = sql_type
            && let Some(field) = plan_spatial_type(name, modifiers)?
        {
            return Ok(Some(field));
        }
        match &self.fallback {
            Some(fallback) => fallback.plan_type_field(sql_type),
            None => Ok(None),
        }
    }
}

fn plan_spatial_type(name: &ast::ObjectName, modifiers: &[String]) -> Result<Option<FieldRef>> {
    // A schema-qualified name, such as public.geometry, names the same type.
    let Some(ObjectNamePart::Identifier(ident)) = name.0.last() else {
        return Ok(None);
    };
    let field = match ident.value.to_ascii_lowercase().as_str() {
        "geometry" => {
            let srid = geometry_typmod_srid(modifiers)?;
            let metadata = Arc::new(Metadata::new(srid_to_crs(srid), None));
            Field::new("", DataType::Binary, true).with_extension_type(WkbType::new(metadata))
        }
        "box2d" if modifiers.is_empty() => box_field(Dimension::XY),
        "box3d" if modifiers.is_empty() => box_field(Dimension::XYZ),
        _ => return Ok(None),
    };
    Ok(Some(Arc::new(field)))
}

fn box_field(dimension: Dimension) -> Field {
    let box_type = BoxType::new(dimension, Default::default());
    Field::new("", box_type.data_type(), true).with_extension_type(box_type)
}

/// The SRID of a `geometry(type[, srid])` type modifier, checking the type as PostGIS does.
fn geometry_typmod_srid(modifiers: &[String]) -> Result<i32> {
    let (geometry_type, srid) = match modifiers {
        [] => return Ok(SRID_UNKNOWN),
        [geometry_type] => (geometry_type, None),
        [geometry_type, srid] => (geometry_type, Some(srid)),
        _ => return plan_err!("Invalid geometry type modifier: ({})", modifiers.join(",")),
    };
    let lower = geometry_type.to_ascii_lowercase();
    let base = ["zm", "z", "m"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix))
        .filter(|base| GEOMETRY_TYPES.contains(base))
        .unwrap_or(&lower);
    if !GEOMETRY_TYPES.contains(&base) {
        return plan_err!("Invalid geometry type modifier: {geometry_type}");
    }
    let Some(srid) = srid else {
        return Ok(SRID_UNKNOWN);
    };
    match srid.trim().parse::<i64>() {
        Ok(srid) => Ok(clamp_srid(srid)),
        Err(_) => plan_err!("Invalid geometry SRID modifier: {srid}"),
    }
}

#[cfg(test)]
mod test {
    use datafusion::sql::sqlparser::dialect::PostgreSqlDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use geoarrow_schema::crs::Crs;

    use super::*;

    fn plan(sql_type: &str) -> Result<Option<FieldRef>> {
        let data_type = Parser::new(&PostgreSqlDialect {})
            .try_with_sql(sql_type)
            .unwrap()
            .parse_data_type()
            .unwrap();
        GeoTypePlanner::new().plan_type_field(&data_type)
    }

    fn crs(sql_type: &str) -> Crs {
        let field = plan(sql_type).unwrap().unwrap();
        field
            .try_extension_type::<WkbType>()
            .unwrap()
            .metadata()
            .crs()
            .clone()
    }

    #[test]
    fn test_geometry_typmod() {
        assert_eq!(crs("geometry"), Crs::default());
        assert_eq!(crs("GEOMETRY(Point)"), Crs::default());
        assert_eq!(crs("public.geometry(PointZ, 4326)"), srid_to_crs(4326));
        assert_eq!(crs("geometry(MultiPolygonZM, 0)"), Crs::default());
        assert!(plan("geometry(Pointy, 4326)").is_err());
        assert!(plan("geometry(Point, x)").is_err());
    }

    #[test]
    fn test_box_types() {
        let field = plan("box3d").unwrap().unwrap();
        let box_type = field.try_extension_type::<BoxType>().unwrap();
        assert_eq!(box_type.dimension(), Dimension::XYZ);
        assert!(plan("text").unwrap().is_none());
    }
}
