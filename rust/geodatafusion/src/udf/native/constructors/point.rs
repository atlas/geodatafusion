//! Point constructors

use std::sync::{Arc, LazyLock};

use arrow_array::cast::AsArray;
use arrow_array::types::Float64Type;
use arrow_array::{Array, ArrayRef, new_null_array};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
    TypeSignature, Volatility,
};
use datafusion_macros::user_doc;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::array::{PointArray, SeparatedCoordBuffer};
use geoarrow_array::builder::PointBuilder;
use geoarrow_schema::{CoordType, Crs, Dimension, Metadata, PointType};

use crate::error::GeoDataFusionResult;
use crate::util::args::scalar_srid;
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
pub struct Point {
    coord_type: CoordType,
}

impl Point {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for Point {
    fn default() -> Self {
        Self::new(Default::default())
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
        point_return_field(self.name(), &args, Dimension::XY, self.coord_type, 2)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINT_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, 2)?)
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
pub struct PointZ {
    coord_type: CoordType,
}

impl PointZ {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for PointZ {
    fn default() -> Self {
        Self::new(Default::default())
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
        point_return_field(self.name(), &args, Dimension::XYZ, self.coord_type, 3)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTZ_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, 3)?)
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
pub struct PointM {
    coord_type: CoordType,
}

impl PointM {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for PointM {
    fn default() -> Self {
        Self::new(Default::default())
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
        point_return_field(self.name(), &args, Dimension::XYM, self.coord_type, 3)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTM_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, 3)?)
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
pub struct PointZM {
    coord_type: CoordType,
}

impl PointZM {
    pub fn new(coord_type: CoordType) -> Self {
        Self { coord_type }
    }
}

impl Default for PointZM {
    fn default() -> Self {
        Self::new(Default::default())
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
        point_return_field(self.name(), &args, Dimension::XYZM, self.coord_type, 4)
    }

    fn coerce_types(&self, arg_types: &[DataType]) -> Result<Vec<DataType>> {
        coerce_args(self.name(), arg_types, POINTZM_ARGUMENTS)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(point_impl(&args, 4)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

/// The return field of a point constructor whose coordinates are followed by an optional
/// constant SRID at `srid_index`.
fn point_return_field(
    name: &str,
    args: &ReturnFieldArgs,
    dim: Dimension,
    coord_type: CoordType,
    srid_index: usize,
) -> Result<FieldRef> {
    let crs = if args.arg_fields.len() > srid_index {
        scalar_srid(name, args, srid_index)?
            .map(srid_to_crs)
            .unwrap_or_default()
    } else {
        Crs::default()
    };
    let typ = PointType::new(dim, Arc::new(Metadata::new(crs, None))).with_coord_type(coord_type);
    Ok(Arc::new(typ.to_field(name, true)))
}

/// Builds the points of a point constructor from its first `coord_count` arguments. A NULL SRID
/// gives NULL in every row.
fn point_impl(args: &ScalarFunctionArgs, coord_count: usize) -> GeoDataFusionResult<ColumnarValue> {
    // SQL NULL in, SQL NULL out.
    if matches!(args.args.get(coord_count), Some(ColumnarValue::Scalar(srid)) if srid.is_null()) {
        let nulls = new_null_array(args.return_field.data_type(), args.number_rows);
        return Ok(ColumnarValue::Array(nulls));
    }
    let arrays = ColumnarValue::values_to_arrays(&args.args[..coord_count])?;
    let point_arr = create_point_array(arrays, &args.return_field)?;
    Ok(point_arr.into_array_ref().into())
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
    coord_type: CoordType,
}

impl MakePoint {
    pub fn new(coord_type: CoordType) -> Self {
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
            coord_type,
        }
    }
}

impl Default for MakePoint {
    fn default() -> Self {
        Self::new(Default::default())
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

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<Arc<Field>> {
        let dim = match args.arg_fields.len() {
            2 => Dimension::XY,
            3 => Dimension::XYZ,
            4 => Dimension::XYZM,
            _ => unreachable!(),
        };

        let typ = PointType::new(dim, Default::default()).with_coord_type(self.coord_type);
        Ok(typ.to_field("", true).into())
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let arrays = ColumnarValue::values_to_arrays(&args.args)?;
        let point_arr = create_point_array(arrays, &args.return_field)?;
        Ok(point_arr.into_array_ref().into())
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
    coord_type: CoordType,
}

impl MakePointM {
    pub fn new(coord_type: CoordType) -> Self {
        Self {
            signature: Signature::exact(
                vec![DataType::Float64, DataType::Float64, DataType::Float64],
                Volatility::Immutable,
            ),
            coord_type,
        }
    }
}

impl Default for MakePointM {
    fn default() -> Self {
        Self::new(Default::default())
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

    fn return_field_from_args(&self, _args: ReturnFieldArgs) -> Result<Arc<Field>> {
        let typ =
            PointType::new(Dimension::XYM, Default::default()).with_coord_type(self.coord_type);
        Ok(typ.to_field("", true).into())
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let arrays = ColumnarValue::values_to_arrays(&args.args)?;
        let point_arr = create_point_array(arrays, &args.return_field)?;
        Ok(point_arr.into_array_ref().into())
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn create_point_array(
    arrays: Vec<ArrayRef>,
    return_field: &Field,
) -> GeoDataFusionResult<PointArray> {
    let x = arrays[0].as_primitive::<Float64Type>();
    let y = arrays[1].as_primitive::<Float64Type>();
    let z = arrays.get(2).map(|arr| arr.as_primitive::<Float64Type>());
    let m = arrays.get(3).map(|arr| arr.as_primitive::<Float64Type>());

    let typ = return_field.extension_type::<PointType>();
    let point_arr = match typ.coord_type() {
        CoordType::Interleaved => {
            let mut builder = PointBuilder::with_capacity(typ, x.len());

            match (z, m) {
                (None, None) => {
                    for (x, y) in x.iter().zip(y.iter()) {
                        if let (Some(x), Some(y)) = (x, y) {
                            let coord = wkt::types::Coord {
                                x,
                                y,
                                z: None,
                                m: None,
                            };
                            builder.push_coord(Some(&coord));
                        } else {
                            builder.push_null();
                        }
                    }
                }
                (Some(z), None) => {
                    for ((x, y), z) in x.iter().zip(y.iter()).zip(z.iter()) {
                        if let (Some(x), Some(y), Some(z)) = (x, y, z) {
                            let coord = wkt::types::Coord {
                                x,
                                y,
                                z: Some(z),
                                m: None,
                            };
                            builder.push_coord(Some(&coord));
                        } else {
                            builder.push_null();
                        }
                    }
                }
                (None, Some(m)) => {
                    for ((x, y), m) in x.iter().zip(y.iter()).zip(m.iter()) {
                        if let (Some(x), Some(y), Some(m)) = (x, y, m) {
                            let coord = wkt::types::Coord {
                                x,
                                y,
                                z: None,
                                m: Some(m),
                            };
                            builder.push_coord(Some(&coord));
                        } else {
                            builder.push_null();
                        }
                    }
                }
                (Some(z), Some(m)) => {
                    for (((x, y), z), m) in x.iter().zip(y.iter()).zip(z.iter()).zip(m.iter()) {
                        if let (Some(x), Some(y), Some(z), Some(m)) = (x, y, z, m) {
                            let coord = wkt::types::Coord {
                                x,
                                y,
                                z: Some(z),
                                m: Some(m),
                            };
                            builder.push_coord(Some(&coord));
                        } else {
                            builder.push_null();
                        }
                    }
                }
            }

            builder.finish()
        }
        CoordType::Separated => {
            let (_, dim, metadata) = typ.into_inner();
            let mut coord_buffers = vec![x.values().clone(), y.values().clone()];
            if let Some(z) = z {
                coord_buffers.push(z.values().clone());
            }
            if let Some(m) = m {
                coord_buffers.push(m.values().clone());
            }

            // A NULL in any coordinate makes the point NULL.
            let nulls = [Some(x), Some(y), z, m]
                .into_iter()
                .flatten()
                .fold(None, |nulls, array| {
                    NullBuffer::union(nulls.as_ref(), array.nulls())
                });

            let coords = SeparatedCoordBuffer::from_vec(coord_buffers, dim)?;
            PointArray::new(coords.into(), nulls, metadata)
        }
    };

    Ok(point_arr)
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use approx::relative_eq;
    use arrow_array::{RecordBatch, create_array};
    use arrow_schema::Schema;
    use datafusion::prelude::SessionContext;
    use geo_traits::{CoordTrait, PointTrait};
    use geoarrow_array::GeoArrowArrayAccessor;

    use super::*;

    #[tokio::test]
    async fn test_st_point() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point::new(CoordType::Separated).into());

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
        let point_arr = PointArray::try_from((output_column.as_ref(), output_field)).unwrap();

        assert_eq!(point_arr.len(), 1);
        let (x, y) = point_arr.value(0).unwrap().coord().unwrap().x_y();

        assert!(relative_eq!(x, -71.104));
        assert!(relative_eq!(y, 42.315));
    }

    #[tokio::test]
    async fn test_st_point_from_table() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point::new(CoordType::Separated).into());

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
        assert_eq!(output_field.extension_type_name(), Some("geoarrow.point"));

        let output_column = output_batch.column(0);
        let point_arr = PointArray::try_from((output_column.as_ref(), output_field)).unwrap();

        assert_eq!(point_arr.len(), 1);
        let (x, y) = point_arr.value(0).unwrap().coord().unwrap().x_y();

        assert!(relative_eq!(x, -71.104));
        assert!(relative_eq!(y, 42.315));
    }

    #[tokio::test]
    async fn test_st_point_srid() {
        let ctx = SessionContext::new();

        ctx.register_udf(Point::new(CoordType::Separated).into());

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
        let point_type = output_field.extension_type::<PointType>();
        assert_eq!(
            point_type.metadata().crs(),
            &Crs::from_authority_code("EPSG:4326".to_string())
        );
    }
}
