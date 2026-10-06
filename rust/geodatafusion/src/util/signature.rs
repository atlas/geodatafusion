//! Signatures and argument coercion for geodatafusion UDFs.

use std::sync::LazyLock;

use arrow_schema::DataType;
use datafusion::common::plan_err;
use datafusion::error::Result;
use datafusion::logical_expr::{Signature, Volatility};
use geoarrow_schema::{
    BoxType, CoordType, Dimension, GeometryCollectionType, GeometryType, LineStringType,
    MultiLineStringType, MultiPointType, MultiPolygonType, PointType, PolygonType,
};

/// Every Arrow type a geometry argument accepts: native GeoArrow types, boxes, WKB and WKT.
pub(crate) fn any_geometry_type() -> &'static [DataType] {
    &ANY_GEOMETRY_TYPE
}

static ANY_GEOMETRY_TYPE: LazyLock<Vec<DataType>> = LazyLock::new(|| {
    let expected_capacity = (2 * 4 * 7) + 2 + 4 + 3 + 3;
    let mut valid_types = Vec::with_capacity(expected_capacity);

    for coord_type in [CoordType::Separated, CoordType::Interleaved] {
        for dim in [
            Dimension::XY,
            Dimension::XYZ,
            Dimension::XYM,
            Dimension::XYZM,
        ] {
            valid_types.push(
                PointType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                LineStringType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                PolygonType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                MultiPointType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                MultiLineStringType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                MultiPolygonType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
            valid_types.push(
                GeometryCollectionType::new(dim, Default::default())
                    .with_coord_type(coord_type)
                    .data_type(),
            );
        }
    }

    for coord_type in [CoordType::Separated, CoordType::Interleaved] {
        valid_types.push(
            GeometryType::new(Default::default())
                .with_coord_type(coord_type)
                .data_type(),
        );
    }

    for dim in [
        Dimension::XY,
        Dimension::XYZ,
        Dimension::XYM,
        Dimension::XYZM,
    ] {
        valid_types.push(BoxType::new(dim, Default::default()).data_type());
    }

    // Wkb
    valid_types.push(DataType::Binary);
    valid_types.push(DataType::LargeBinary);
    valid_types.push(DataType::BinaryView);

    // Wkt
    valid_types.push(DataType::Utf8);
    valid_types.push(DataType::LargeUtf8);
    valid_types.push(DataType::Utf8View);

    debug_assert_eq!(valid_types.len(), expected_capacity);

    valid_types
});

static SINGLE_GEOMETRY: LazyLock<Signature> =
    LazyLock::new(|| Signature::uniform(1, any_geometry_type().to_vec(), Volatility::Immutable));

/// The signature of a function whose only argument is a geometry.
pub(crate) fn single_geometry() -> &'static Signature {
    &SINGLE_GEOMETRY
}

/// A PostGIS argument type, for [`coerce_args`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arg {
    /// `geometry` (or `box2d`/`box3d`): any of [`any_geometry_type`], kept as is so the field
    /// metadata survives. `Null` becomes `Binary`, a NULL WKB value.
    Geometry,
    /// `float8`: numeric types and `Null` become `Float64`.
    Float,
    /// `integer`: integer types and `Null` become `Int32`.
    Integer,
    /// An `integer` SRID. Integer types and `Null` are kept as is, so that a constant stays a
    /// literal for `return_field_from_args`: coercing it would wrap it in a cast.
    Srid,
    /// `text`: string types are kept as is. `Null` becomes `Utf8`.
    Text,
    /// `boolean`: `Boolean` and `Null` become `Boolean`.
    #[cfg_attr(
        all(not(feature = "geos-3_11"), not(test)),
        expect(dead_code, reason = "only GEOS-backed UDFs take a boolean so far")
    )]
    Boolean,
}

impl Arg {
    /// The type `data_type` is coerced to, or `None` if this argument doesn't accept it.
    fn coerce(self, data_type: &DataType) -> Option<DataType> {
        use DataType::*;

        match (self, data_type) {
            (Arg::Geometry, Null) => Some(Binary),
            (Arg::Geometry, t) if any_geometry_type().contains(t) => Some(t.clone()),
            (Arg::Float, t) if t.is_numeric() || t.is_null() => Some(Float64),
            (Arg::Integer, t) if t.is_integer() || t.is_null() => Some(Int32),
            (Arg::Srid, Null) => Some(Null),
            (Arg::Srid, t) if t.is_integer() => Some(t.clone()),
            (Arg::Text, Null) => Some(Utf8),
            (Arg::Text, Utf8 | LargeUtf8 | Utf8View) => Some(data_type.clone()),
            (Arg::Boolean, Boolean | Null) => Some(Boolean),
            _ => None,
        }
    }

    /// The PostGIS name of this argument type, for error messages.
    fn sql_name(self) -> &'static str {
        match self {
            Arg::Geometry => "geometry",
            Arg::Float => "float8",
            Arg::Integer => "integer",
            Arg::Srid => "integer",
            Arg::Text => "text",
            Arg::Boolean => "boolean",
        }
    }
}

/// `coerce_types` for a [`Signature::user_defined`] UDF: the coerced types of the first overload
/// in `overloads` that `arg_types` match, otherwise a plan error naming `name`.
///
/// Each overload lists a PostGIS signature's argument types, so a UDF's overloads read like its
/// PostGIS synopsis. PostGIS `DEFAULT` parameters become shorter overloads.
pub(crate) fn coerce_args(
    name: &str,
    arg_types: &[DataType],
    overloads: &[&[Arg]],
) -> Result<Vec<DataType>> {
    for overload in overloads {
        if overload.len() != arg_types.len() {
            continue;
        }
        let coerced: Option<Vec<DataType>> = overload
            .iter()
            .zip(arg_types)
            .map(|(arg, data_type)| arg.coerce(data_type))
            .collect();
        if let Some(coerced) = coerced {
            return Ok(coerced);
        }
    }
    let supported = overloads
        .iter()
        .map(|overload| {
            let args: Vec<&str> = overload.iter().map(|arg| arg.sql_name()).collect();
            format!("{name}({})", args.join(", "))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let given: Vec<String> = arg_types.iter().map(|t| t.to_string()).collect();
    plan_err!(
        "{name} does not support arguments ({}). Supported: {supported}",
        given.join(", ")
    )
}

#[cfg(test)]
mod test {
    use super::*;

    const OVERLOADS: &[&[Arg]] = &[
        &[Arg::Geometry, Arg::Float],
        &[Arg::Geometry, Arg::Float, Arg::Boolean],
    ];

    #[test]
    fn test_coerce_args_picks_matching_overload() {
        let wkb = DataType::Binary;
        let coerced =
            coerce_args("st_simplify", &[wkb.clone(), DataType::Int64], OVERLOADS).unwrap();
        assert_eq!(coerced, vec![wkb.clone(), DataType::Float64]);

        let coerced = coerce_args(
            "st_simplify",
            &[DataType::Null, DataType::Float32, DataType::Null],
            OVERLOADS,
        )
        .unwrap();
        assert_eq!(
            coerced,
            vec![DataType::Binary, DataType::Float64, DataType::Boolean]
        );
    }

    #[test]
    fn test_coerce_args_keeps_srid_integer_type() {
        let coerced = coerce_args(
            "st_setsrid",
            &[DataType::Binary, DataType::Int64],
            &[&[Arg::Geometry, Arg::Srid]],
        )
        .unwrap();
        assert_eq!(coerced, vec![DataType::Binary, DataType::Int64]);

        let coerced = coerce_args(
            "st_setsrid",
            &[DataType::Binary, DataType::Null],
            &[&[Arg::Geometry, Arg::Srid]],
        )
        .unwrap();
        assert_eq!(coerced, vec![DataType::Binary, DataType::Null]);
    }

    #[test]
    fn test_coerce_args_rejects_unsupported_types() {
        let err = coerce_args(
            "st_simplify",
            &[DataType::Binary, DataType::Utf8],
            OVERLOADS,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("st_simplify does not support arguments (Binary, Utf8)"),
            "{err}"
        );
        assert!(
            err.contains("st_simplify(geometry, float8, boolean)"),
            "{err}"
        );
    }
}
