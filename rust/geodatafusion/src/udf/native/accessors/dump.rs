use std::sync::Arc;

use arrow_schema::{DataType, FieldRef};
use datafusion::common::internal_err;
use datafusion::error::Result;
use datafusion::logical_expr::{
    ColumnarValue, Documentation, ReturnFieldArgs, ScalarFunctionArgs, ScalarUDFImpl, Signature,
};
use datafusion_macros::user_doc;
use geo_traits::{
    GeometryCollectionTrait, GeometryTrait, GeometryType, MultiLineStringTrait, MultiPointTrait,
    MultiPolygonTrait,
};

use crate::error::GeoDataFusionResult;
use crate::udf::native::accessors::is_empty::is_geometry_topologically_empty;
use crate::udf::native::accessors::util::geometry_dump::{
    GeometryDump, geometry_dump_array, geometry_dump_field,
};
use crate::util::field::geometry_array;
use crate::util::kernel::{GeometryKernel, map_geometry};
use crate::util::owned::to_owned_geometry;
use crate::util::signature::single_geometry;

/// Returns a set of geometry_dump rows for the components of a geometry.
#[user_doc(
    doc_section(label = "Geometry Accessors"),
    description = "A set-returning function that extracts the components of a geometry: the points, lines and polygons of MULTI geometries and collections, recursively, each with its path, the 1-based indexes leading to it. An atomic geometry is returned whole with an empty path. An empty geometry has no components, but an empty part of a non-empty collection is one. Returns a list of (path, geom) records per row: unnest it for rows, or call st_dump(geom) in FROM with a constant argument.",
    syntax_example = "ST_Dump(g1)",
    argument(name = "g1", description = "geometry"),
    related_udf(name = "st_dumppoints"),
    related_udf(name = "st_dumprings"),
    related_udf(name = "st_dumpsegments")
)]
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Dump;

impl Dump {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Dump {
    fn default() -> Self {
        Self::new()
    }
}

impl ScalarUDFImpl for Dump {
    fn name(&self) -> &str {
        "st_dump"
    }

    fn signature(&self) -> &Signature {
        single_geometry()
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        internal_err!("return_field_from_args should be called instead")
    }

    fn return_field_from_args(&self, args: ReturnFieldArgs) -> Result<FieldRef> {
        Ok(geometry_dump_field(self.name(), &args.arg_fields[0]))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        Ok(dump_impl(args)?)
    }

    fn documentation(&self) -> Option<&Documentation> {
        self.doc()
    }
}

fn dump_impl(args: ScalarFunctionArgs) -> GeoDataFusionResult<ColumnarValue> {
    let geometries = geometry_array(&args, 0)?;
    let rows: Vec<Option<GeometryDump>> = map_geometry(geometries.as_ref(), &DumpKernel)?;
    let result = geometry_dump_array(rows, &args.return_field)?;
    Ok(ColumnarValue::Array(Arc::new(result)))
}

struct DumpKernel;

impl GeometryKernel for DumpKernel {
    type Output = GeometryDump;

    fn eval(
        &self,
        geom: &impl GeometryTrait<T = f64>,
        _row: usize,
    ) -> GeoDataFusionResult<Option<GeometryDump>> {
        let mut dump = vec![];
        // A geometry that is empty all the way down has no components.
        if !is_geometry_topologically_empty(geom) {
            dump_geometry(geom, &mut vec![], &mut dump);
        }
        Ok(Some(dump))
    }
}

/// Adds the atomic components of `geom`, under `path`.
fn dump_geometry(geom: &impl GeometryTrait<T = f64>, path: &mut Vec<i32>, dump: &mut GeometryDump) {
    match geom.as_type() {
        GeometryType::MultiPoint(points) => dump_parts(points.points(), path, dump),
        GeometryType::MultiLineString(lines) => dump_parts(lines.line_strings(), path, dump),
        GeometryType::MultiPolygon(polygons) => dump_parts(polygons.polygons(), path, dump),
        GeometryType::GeometryCollection(collection) => {
            dump_parts(collection.geometries(), path, dump)
        }
        _ => dump.push((path.clone(), to_owned_geometry(geom))),
    }
}

fn dump_parts(
    parts: impl Iterator<Item = impl GeometryTrait<T = f64>>,
    path: &mut Vec<i32>,
    dump: &mut GeometryDump,
) {
    for (index, part) in (1..).zip(parts) {
        path.push(index);
        dump_geometry(&part, path, dump);
        path.pop();
    }
}

#[cfg(test)]
mod test {
    use arrow_array::Array;
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int32Type;
    use arrow_schema::DataType;
    use datafusion::prelude::SessionContext;
    use geoarrow_array::GeoArrowArray;
    use geoarrow_array::array::from_arrow_array;
    use geoarrow_array::cast::to_wkt;

    use super::*;
    use crate::udf::native::io::{AsText, GeomFromText};

    fn ctx() -> SessionContext {
        let ctx = SessionContext::new();
        ctx.register_udf(Dump.into());
        ctx.register_udf(GeomFromText::default().into());
        ctx.register_udf(AsText.into());
        ctx
    }

    /// Run `ST_Dump` on a single WKT input and return the result decoded as WKT.
    async fn dump_rows(ctx: &SessionContext, wkt: &str) -> Vec<(Vec<i32>, String)> {
        let sql = format!("SELECT ST_Dump(ST_GeomFromText('{wkt}'))");
        let df = ctx.sql(&sql).await.unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let list = batch.column(0).as_list::<i32>();
        assert!(!list.is_null(0), "unexpected NULL dump for {wkt}");

        let components = list.value(0);
        let structs = components.as_struct();
        let path_col = structs.column(0).as_list::<i32>();

        let DataType::Struct(fields) = structs.data_type() else {
            panic!("ST_Dump must return a list of structs");
        };
        let geo = from_arrow_array(structs.column(1).as_ref(), fields[1].as_ref()).unwrap();
        let wkt_ref = to_wkt::<i32>(geo.as_ref()).unwrap().into_array_ref();
        let wkts = wkt_ref.as_string::<i32>();

        (0..structs.len())
            .map(|i| {
                let path = path_col
                    .value(i)
                    .as_primitive::<Int32Type>()
                    .values()
                    .to_vec();
                (path, wkts.value(i).to_string())
            })
            .collect()
    }

    #[tokio::test]
    async fn test_atomic_round_trips() {
        let ctx = ctx();
        assert_eq!(
            dump_rows(&ctx, "POINT(1 2)").await,
            vec![(vec![], "POINT(1 2)".to_string())],
            "Point is atomic and should round-trip to itself.",
        );

        assert_eq!(
            dump_rows(&ctx, "LINESTRING(0 0,1 1)").await,
            vec![(vec![], "LINESTRING(0 0,1 1)".to_string())],
            "LineString is atomic and should round-trip to itself.",
        );

        assert_eq!(
            dump_rows(&ctx, "POLYGON((0 0,0 3,3 3,3 0,0 0),(1 1,1 2,2 2,2 1,1 1))").await,
            vec![(
                vec![],
                "POLYGON((0 0,0 3,3 3,3 0,0 0),(1 1,1 2,2 2,2 1,1 1))".to_string()
            )],
            "Polygon is atomic and should round-trip to itself.",
        );
    }

    #[tokio::test]
    async fn test_multi_geometries_decompose() {
        let ctx = ctx();
        assert_eq!(
            dump_rows(&ctx, "MULTIPOINT(0 0,1 1)").await,
            vec![
                (vec![1], "POINT(0 0)".to_string()),
                (vec![2], "POINT(1 1)".to_string()),
            ],
        );

        assert_eq!(
            dump_rows(&ctx, "MULTILINESTRING((0 0,1 1),(2 2,3 3))").await,
            vec![
                (vec![1], "LINESTRING(0 0,1 1)".to_string()),
                (vec![2], "LINESTRING(2 2,3 3)".to_string()),
            ],
        );

        assert_eq!(
            dump_rows(
                &ctx,
                "MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)),((10 10,10 20,20 20,20 10,10 10)))"
            )
            .await,
            vec![
                (vec![1], "POLYGON((0 0,0 1,1 1,1 0,0 0))".to_string()),
                (
                    vec![2],
                    "POLYGON((10 10,10 20,20 20,20 10,10 10))".to_string()
                ),
            ],
        );
    }

    #[tokio::test]
    async fn test_collection_decomposition() {
        let ctx = ctx();
        assert_eq!(
            dump_rows(&ctx, "GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(1 1,2 2))").await,
            vec![
                (vec![1], "POINT(0 0)".to_string()),
                (vec![2], "LINESTRING(1 1,2 2)".to_string()),
            ],
            "GeometryCollection should be decomposed into components"
        );

        assert_eq!(
            dump_rows(
                &ctx,
                "GEOMETRYCOLLECTION(MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)),((10 10,10 20,20 20,20 10,10 10))),POINT(5 5))"
            )
            .await,
            vec![
                (vec![1, 1], "POLYGON((0 0,0 1,1 1,1 0,0 0))".to_string()),
                (vec![1, 2], "POLYGON((10 10,10 20,20 20,20 10,10 10))".to_string()),
                (vec![2], "POINT(5 5)".to_string()),
            ],
            "Nesting a MultiPolygon in a GeometryCollection should unroll into atomic Polygons"
        );
    }

    #[tokio::test]
    async fn test_dimension_preservation() {
        let ctx = ctx();
        assert_eq!(
            dump_rows(&ctx, "MULTIPOINT Z (0 0 1,1 1 2)").await,
            vec![
                (vec![1], "POINT Z(0 0 1)".to_string()),
                (vec![2], "POINT Z(1 1 2)".to_string()),
            ],
        );
        assert_eq!(
            dump_rows(&ctx, "MULTIPOINT ZM (0 0 1 5,1 1 2 6)").await,
            vec![
                (vec![1], "POINT ZM(0 0 1 5)".to_string()),
                (vec![2], "POINT ZM(1 1 2 6)".to_string()),
            ],
        );
    }

    #[tokio::test]
    async fn test_dump_empty_inputs_yield_zero_rows() {
        let ctx = ctx();
        assert_eq!(dump_rows(&ctx, "POINT EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "LINESTRING EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "POLYGON EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "MULTIPOINT EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "MULTILINESTRING EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "MULTIPOLYGON EMPTY").await, vec![]);
        assert_eq!(dump_rows(&ctx, "GEOMETRYCOLLECTION EMPTY").await, vec![]);
    }

    #[tokio::test]
    async fn test_dump_recursive_empty_collection_yields_zero_rows() {
        let ctx = ctx();
        let wkt = "GEOMETRYCOLLECTION(POINT EMPTY, LINESTRING EMPTY, POLYGON EMPTY, MULTIPOINT EMPTY, MULTILINESTRING EMPTY, MULTIPOLYGON EMPTY)";
        assert_eq!(dump_rows(&ctx, wkt).await, vec![]);
    }

    #[tokio::test]
    async fn test_dump_mixed_empty_and_nonempty_children_emits_empty_leaves() {
        let ctx = ctx();
        assert_eq!(
            dump_rows(&ctx, "GEOMETRYCOLLECTION(POINT(0 0), POLYGON EMPTY)").await,
            vec![
                (vec![1], "POINT(0 0)".to_string()),
                (vec![2], "POLYGON EMPTY".to_string()),
            ],
            "A single non-empty geometry in a collection should make the collection non-empty, including empty children"
        );
    }

    #[tokio::test]
    async fn test_dump_null_yields_null() {
        let ctx = ctx();
        let df = ctx
            .sql("SELECT ST_Dump(ST_GeomFromText(NULL))")
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();
        let list = batch.column(0).as_list::<i32>();
        assert!(list.is_null(0), "ST_Dump(NULL) should be NULL");
    }

    #[tokio::test]
    async fn test_unnest_expands_to_multiple_rows() {
        let ctx = ctx();
        let df = ctx
            .sql("SELECT unnest(ST_Dump(ST_GeomFromText('MULTIPOINT(0 0,1 1)'))) AS comp")
            .await
            .unwrap();
        let batches = df.collect().await.unwrap();
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(
            rows, 2,
            "unnest should expand a 2-component dump into 2 rows"
        );
        assert!(matches!(
            batches[0].column(0).data_type(),
            DataType::Struct(_)
        ));
    }

    #[tokio::test]
    async fn test_st_dump_field_access_sql() {
        let ctx = ctx();
        let df = ctx
            .sql(
                // This SQL illustrates how to get a PostGIS-style shape with multiple rows per non-atomic geometry.
                "SELECT ST_AsText(comp.geom) AS wkt, comp.path AS path \
                 FROM (SELECT unnest(ST_Dump(ST_GeomFromText('MULTIPOINT(0 0,1 1)'))) AS comp)",
            )
            .await
            .unwrap();
        let batch = df.collect().await.unwrap().into_iter().next().unwrap();

        // `comp['geom']` keeps the geometry's extension type, so ST_AsText accepts it directly.
        let wkt = batch.column_by_name("wkt").unwrap().as_string::<i32>();
        assert_eq!(
            (0..wkt.len()).map(|i| wkt.value(i)).collect::<Vec<_>>(),
            vec!["POINT(0 0)", "POINT(1 1)"],
        );

        // `comp['path']` extracts the navigation-path list (a single 1-based index for each part).
        let path = batch.column_by_name("path").unwrap().as_list::<i32>();
        assert_eq!(path.len(), 2);
        assert_eq!(path.value(0).as_primitive::<Int32Type>().values(), &[1]);
        assert_eq!(path.value(1).as_primitive::<Int32Type>().values(), &[2]);
    }
}
