//! Point constructors

use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use arrow_array::{Array, ArrayRef, new_null_array};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::{internal_datafusion_err, internal_err};
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    TypeSignature, Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_array::capacity::WkbCapacity;
use geoarrow_schema::{Crs, Dimension, GeoArrowType, Metadata};
use wkt::types::Coord;

use crate::error::GeoDataFusionResult;
use crate::util::args::scalar_srid;
use crate::util::field::wkb_return_field;
use crate::util::signature::{Arg, coerce_args};
use crate::util::srid::srid_to_crs;

/// PostGIS: ST_Point(float8 x, float8 y, integer srid = unknown).
static POINT_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Float],
    &[Arg::Float, Arg::Float, Arg::Srid],
];

static POINT_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["x", "y", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a Point with X, Y and SRID values.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a Point with X, Y and SRID values. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_Point(x, y, srid)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_makepoint"),
    related_udf(name = "st_pointz")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Point;

impl Point {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Point {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Point {
    fn name(&self) -> &str {
        "st_point"
    }

    fn signature(&self) -> &Signature {
        &POINT_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        point_return_field(self.name(), &args, 2)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, Dimension::XY)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: ST_PointZ(float8 x, float8 y, float8 z, integer srid = unknown).
static POINTZ_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Srid],
];

static POINTZ_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["x", "y", "z", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a Point with X, Y, Z and SRID values.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a Point with X, Y, Z and SRID values. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_PointZ(x, y, z, srid)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "z", description = "float8"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_point"),
    related_udf(name = "st_makepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointZ;

impl PointZ {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointZ {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointZ {
    fn name(&self) -> &str {
        "st_pointz"
    }

    fn signature(&self) -> &Signature {
        &POINTZ_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        point_return_field(self.name(), &args, 3)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTZ_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, Dimension::XYZ)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: ST_PointM(float8 x, float8 y, float8 m, integer srid = unknown).
static POINTM_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Srid],
];

static POINTM_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["x", "y", "m", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a Point with X, Y, M and SRID values.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a Point with X, Y, M and SRID values. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_PointM(x, y, m, srid)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "m", description = "float8"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_point"),
    related_udf(name = "st_makepointm")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointM;

impl PointM {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointM {
    fn name(&self) -> &str {
        "st_pointm"
    }

    fn signature(&self) -> &Signature {
        &POINTM_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        point_return_field(self.name(), &args, 3)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTM_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, Dimension::XYM)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// PostGIS: ST_PointZM(float8 x, float8 y, float8 z, float8 m, integer srid = unknown).
static POINTZM_ARGUMENTS: &[&[Arg]] = &[
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Float],
    &[Arg::Float, Arg::Float, Arg::Float, Arg::Float, Arg::Srid],
];

static POINTZM_SIGNATURE: LazyLock<Signature> = LazyLock::new(|| {
    Signature::user_defined(Volatility::Immutable)
        .with_parameter_names(vec!["x", "y", "z", "m", "srid"])
        .expect("parameter names are valid for a user-defined signature")
});

/// Creates a Point with X, Y, Z, M and SRID values.
#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a Point with X, Y, Z, M and SRID values. The SRID must be a constant, because geodatafusion stores one CRS per column.",
    syntax_example = "ST_PointZM(x, y, z, m, srid)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "z", description = "float8"),
    argument(name = "m", description = "float8"),
    argument(name = "srid", description = "integer"),
    related_udf(name = "st_point"),
    related_udf(name = "st_makepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PointZM;

impl PointZM {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PointZM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for PointZM {
    fn name(&self) -> &str {
        "st_pointzm"
    }

    fn signature(&self) -> &Signature {
        &POINTZM_SIGNATURE
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        point_return_field(self.name(), &args, 4)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTZM_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, Dimension::XYZM)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The return field of a point constructor whose coordinates are followed by an optional
/// constant SRID at `srid_index`.
fn point_return_field(name: &str, args: &ReturnFieldArgs, srid_index: usize) -> Result<FieldRef> {
    let crs = if args.arg_fields.len() > srid_index {
        scalar_srid(name, args, srid_index)?
            .map(srid_to_crs)
            .unwrap_or_default()
    } else {
        Crs::default()
    };
    Ok(wkb_return_field(name, Arc::new(Metadata::new(crs, None))))
}

/// Builds the points of a point constructor from its coordinate arguments. A NULL SRID gives
/// NULL in every row.
fn point_impl(args: &ScalarFunctionArgs, dim: Dimension) -> GeoDataFusionResult<ColumnarValue> {
    let coord_count = dim.size();
    // SQL NULL in, SQL NULL out.
    if matches!(args.args.get(coord_count), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let arrays = ColumnarValue::values_to_arrays(&args.args[..coord_count])?;
    Ok(ColumnarValue::Array(create_point_array(
        &arrays,
        dim,
        &args.return_field,
    )?))
}

#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a 2D XY or 3D XYZ or 4D XYZM Point geometry. Use ST_MakePointM to make points with XYM coordinates",
    syntax_example = "ST_MakePoint(-71.104, 42.315)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "z", description = "float8"),
    argument(name = "m", description = "float8"),
    related_udf(name = "st_point"),
    related_udf(name = "st_pointz"),
    related_udf(name = "st_makepointm")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakePoint {
    signature: Signature,
}

impl MakePoint {
    pub fn new() -> Self {
        Self {
            signature: Signature::one_of(
                vec![
                    TypeSignature::Exact(vec![DataType::Float64, DataType::Float64]),
                    TypeSignature::Exact(vec![
                        DataType::Float64,
                        DataType::Float64,
                        DataType::Float64,
                    ]),
                    TypeSignature::Exact(vec![
                        DataType::Float64,
                        DataType::Float64,
                        DataType::Float64,
                        DataType::Float64,
                    ]),
                ],
                Volatility::Immutable,
            ),
        }
    }
}

impl Default for MakePoint {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakePoint {
    fn name(&self) -> &str {
        "st_makepoint"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Default::default()))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let dim = match args.args.len() {
            2 => Dimension::XY,
            3 => Dimension::XYZ,
            4 => Dimension::XYZM,
            n => return internal_err!("st_makepoint: unexpected {n} arguments"),
        };
        Ok(point_impl(&args, dim)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

#[user_doc(
    doc_section(label = "Geometry Constructors"),
    description = "Creates a point with X, Y and M (measure) ordinates. Use ST_MakePoint to make points with XY, XYZ, or XYZM coordinates.",
    syntax_example = "ST_MakePointM(-71.104, 42.315, 10)",
    argument(name = "x", description = "float8"),
    argument(name = "y", description = "float8"),
    argument(name = "m", description = "float8"),
    related_udf(name = "st_point"),
    related_udf(name = "st_pointz"),
    related_udf(name = "st_makepoint")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct MakePointM {
    signature: Signature,
}

impl MakePointM {
    pub fn new() -> Self {
        Self {
            signature: Signature::exact(
                vec![DataType::Float64, DataType::Float64, DataType::Float64],
                Volatility::Immutable,
            ),
        }
    }
}

impl Default for MakePointM {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for MakePointM {
    fn name(&self) -> &str {
        "st_makepointm"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(wkb_return_field(self.name(), Default::default()))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, Dimension::XYM)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// Writes WKB points from coordinate arrays: X, Y, then Z and M as `dim` has them. A NULL in
/// any coordinate makes the point NULL.
fn create_point_array(
    arrays: &[ArrayRef],
    dim: Dimension,
    return_field: &Field,
) -> GeoDataFusionResult<ArrayRef> {
    let coords: Vec<_> = arrays
        .iter()
        .map(|array| array.as_primitive::<Float64Type>())
        .collect();
    let (has_z, has_m) = match dim {
        Dimension::XY => (false, false),
        Dimension::XYZ => (true, false),
        Dimension::XYM => (false, true),
        Dimension::XYZM => (true, true),
    };
    let len = coords[0].len();

    let GeoArrowType::Wkb(output_type) = GeoArrowType::from_arrow_field(return_field)? else {
        return Err(internal_datafusion_err!("unexpected return field {return_field:?}").into());
    };
    // Byte order, geometry type and the ordinates.
    let point_size = 1 + 4 + 8 * dim.size();
    let capacity = WkbCapacity::new(point_size * len, len);
    let mut builder = WkbBuilder::<i32>::with_capacity(output_type, capacity);
    for row in 0..len {
        let point = (!coords.iter().any(|array| array.is_null(row))).then(|| {
            let mut ordinates = coords.iter().map(|array| array.value(row));
            let coord = Coord {
                x: ordinates.next().unwrap_or_default(),
                y: ordinates.next().unwrap_or_default(),
                z: has_z.then(|| ordinates.next().unwrap_or_default()),
                m: has_m.then(|| ordinates.next().unwrap_or_default()),
            };
            wkt::types::Point::from_coord(coord)
        });
        builder.push_geometry(point.as_ref())?;
    }
    Ok(builder.finish().into_array_ref())
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use approx::relative_eq;
    use arrow_array::{RecordBatch, create_array};
    use arrow_schema::Schema;
    use datafusion::prelude::SessionContext;
    use geo_traits::{CoordTrait, GeometryTrait, GeometryType, PointTrait};
    use geoarrow_array::GeoArrowArrayAccessor;
    use geoarrow_array::array::WkbArray;
    use geoarrow_schema::WkbType;

    use super::*;

    #[tokio::test]
    async fn test_st_point() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point.into());

        let sql_df = ctx
            .sql(r#"SELECT ST_Point(-71.104, 42.315);"#)
            .await
            .unwrap();

        let output_batches = sql_df.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);

        let output_column = output_batch.column(0);
        let point_arr = WkbArray::try_from((output_column.as_ref(), output_field)).unwrap();

        assert_eq!(point_arr.len(), 1);
        let geom = point_arr.value(0).unwrap();
        let GeometryType::Point(point) = geom.as_type() else {
            panic!("expected a point");
        };
        let (x, y) = point.coord().unwrap().x_y();

        assert!(relative_eq!(x, -71.104));
        assert!(relative_eq!(y, 42.315));
    }

    #[tokio::test]
    async fn test_st_point_from_table() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point.into());

        let x = create_array!(Float64, [-71.104]);
        let y = create_array!(Float64, [42.315]);

        let schema = Schema::new([
            Arc::new(Field::new("x", x.data_type().clone(), true)),
            Arc::new(Field::new("y", y.data_type().clone(), true)),
        ]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![x, y]).unwrap();

        ctx.register_batch("t", batch).unwrap();

        let sql_df = ctx.sql(r#"SELECT ST_Point(x, y) from t;"#).await.unwrap();

        let output_batches = sql_df.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);

        // This succeeds
        assert_eq!(output_field.extension_type_name(), Some("geoarrow.wkb"));

        let output_column = output_batch.column(0);
        let point_arr = WkbArray::try_from((output_column.as_ref(), output_field)).unwrap();

        assert_eq!(point_arr.len(), 1);
        let geom = point_arr.value(0).unwrap();
        let GeometryType::Point(point) = geom.as_type() else {
            panic!("expected a point");
        };
        let (x, y) = point.coord().unwrap().x_y();

        assert!(relative_eq!(x, -71.104));
        assert!(relative_eq!(y, 42.315));
    }

    #[tokio::test]
    async fn test_st_point_srid() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point.into());

        let x = create_array!(Float64, [-71.104]);
        let y = create_array!(Float64, [42.315]);

        let schema = Schema::new([
            Arc::new(Field::new("x", x.data_type().clone(), true)),
            Arc::new(Field::new("y", y.data_type().clone(), true)),
        ]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![x, y]).unwrap();

        ctx.register_batch("t", batch).unwrap();

        let sql_df = ctx
            .sql(r#"SELECT ST_Point(x, y, 4326) as geometry from t;"#)
            .await
            .unwrap();

        let output_batches = sql_df.collect().await.unwrap();
        assert_eq!(output_batches.len(), 1);
        let output_batch = &output_batches[0];
        let output_schema = output_batch.schema();
        let output_field = output_schema.field(0);
        let wkb_type = output_field.extension_type::<WkbType>();
        assert_eq!(
            wkb_type.metadata().crs(),
            &Crs::from_authority_code("EPSG:4326".to_string())
        );
    }
}
