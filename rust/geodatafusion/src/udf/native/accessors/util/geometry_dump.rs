//! PostGIS's `geometry_dump` record, `(path integer[], geom geometry)`, as the ST_Dump* functions
//! return it: a `List<Struct<path: List<Int32>, geom: geometry>>` per input row.

use std::sync::Arc;

use arrow_array::builder::{Int32Builder, ListBuilder};
use arrow_array::{ArrayRef, ListArray, StructArray};
use arrow_buffer::{NullBufferBuilder, OffsetBuffer};
use arrow_schema::{DataType, Field, FieldRef, Fields};
use datafusion::common::internal_datafusion_err;
use geoarrow_array::GeoArrowArray;
use geoarrow_array::builder::WkbBuilder;
use geoarrow_schema::{GeoArrowType, WkbType};
use wkt::Wkt;

use crate::error::GeoDataFusionResult;
use crate::util::field::input_metadata;

/// The parts of one input geometry: each part's path and geometry.
pub(crate) type GeometryDump = Vec<(Vec<i32>, Wkt<f64>)>;

/// The field of a `geometry_dump` list, named after the UDF, with the input's CRS on `geom`.
pub(crate) fn geometry_dump_field(name: &str, input: &Field) -> FieldRef {
    let geom_type = WkbType::new(input_metadata(input));
    Arc::new(Field::new(
        name,
        DataType::List(item_field(&geom_type)),
        true,
    ))
}

fn path_item_field() -> FieldRef {
    Arc::new(Field::new("item", DataType::Int32, false))
}

fn record_fields(geom_type: &WkbType) -> Fields {
    let path = Field::new("path", DataType::List(path_item_field()), false);
    let geom = Field::new("geom", DataType::Binary, true).with_extension_type(geom_type.clone());
    Fields::from(vec![path, geom])
}

fn item_field(geom_type: &WkbType) -> FieldRef {
    Arc::new(Field::new(
        "item",
        DataType::Struct(record_fields(geom_type)),
        false,
    ))
}

/// The `geometry_dump` lists of the rows, NULL where the row is `None`, typed by `return_field`.
pub(crate) fn geometry_dump_array(
    rows: Vec<Option<GeometryDump>>,
    return_field: &Field,
) -> GeoDataFusionResult<ListArray> {
    let DataType::List(item) = return_field.data_type() else {
        return Err(internal_datafusion_err!("a geometry_dump field holds a list").into());
    };
    let DataType::Struct(fields) = item.data_type() else {
        return Err(internal_datafusion_err!("a geometry_dump item is a struct").into());
    };
    let GeoArrowType::Wkb(geom_type) = GeoArrowType::from_arrow_field(&fields[1])? else {
        return Err(internal_datafusion_err!("a geometry_dump geom is WKB").into());
    };
    let parts: usize = rows.iter().flatten().map(Vec::len).sum();
    let mut geoms = WkbBuilder::<i32>::new(geom_type.clone());
    let mut paths =
        ListBuilder::with_capacity(Int32Builder::new(), parts).with_field(path_item_field());
    let mut lengths = Vec::with_capacity(rows.len());
    let mut validity = NullBufferBuilder::new(rows.len());
    for row in &rows {
        match row {
            None => {
                validity.append_null();
                lengths.push(0);
            }
            Some(dump) => {
                validity.append_non_null();
                lengths.push(dump.len());
                for (path, geom) in dump {
                    paths.append_value(path.iter().map(|&i| Some(i)));
                    geoms.push_geometry(Some(geom))?;
                }
            }
        }
    }
    let records = StructArray::new(
        record_fields(&geom_type),
        vec![
            Arc::new(paths.finish()) as ArrayRef,
            geoms.finish().into_array_ref(),
        ],
        None,
    );
    Ok(ListArray::new(
        Arc::clone(item),
        OffsetBuffer::<i32>::from_lengths(lengths),
        Arc::new(records),
        validity.finish(),
    ))
}
