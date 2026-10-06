//! H3b: do GeoArrow-tagged Utf8/Binary outputs (ST_AsText -> geoarrow.wkt, ST_AsBinary ->
//! geoarrow.wkb) break DataFusion string/binary functions or export?
//!
//! Every query is run twice: on the tagged output, and on the same value with the tag removed
//! (`arrow_cast(.., 'Utf8')` / `arrow_cast(.., 'Binary')` drop field metadata? -> checked, and
//! as a second control the plain literal). The output field (type + extension tag) is recorded,
//! because a function that "works" but keeps a geoarrow.* tag on a value that is no longer a
//! geometry is also a defect.

use e4_type_model::{context, run};

const T: &str = "ST_AsText(wkb4326)";
const B: &str = "ST_AsBinary(wkb4326)";

fn queries() -> Vec<(&'static str, String)> {
    vec![
        ("output of ST_AsText", format!("SELECT {T} FROM t")),
        ("output of ST_AsBinary", format!("SELECT {B} FROM t")),
        ("|| literal", format!("SELECT {T} || ' x' FROM t")),
        ("literal ||", format!("SELECT 'x ' || {T} FROM t")),
        ("|| itself", format!("SELECT {T} || {T} FROM t")),
        ("concat", format!("SELECT concat({T}, ' x') FROM t")),
        ("concat_ws", format!("SELECT concat_ws(',', {T}, 'x') FROM t")),
        ("LIKE", format!("SELECT {T} LIKE 'POINT%' FROM t")),
        ("ILIKE", format!("SELECT {T} ILIKE 'point%' FROM t")),
        ("~ regex", format!("SELECT {T} ~ '^POINT' FROM t")),
        ("length", format!("SELECT length({T}) FROM t")),
        ("char_length", format!("SELECT char_length({T}) FROM t")),
        ("upper", format!("SELECT upper({T}) FROM t")),
        ("lower", format!("SELECT lower({T}) FROM t")),
        ("substr", format!("SELECT substr({T}, 1, 5) FROM t")),
        ("replace", format!("SELECT replace({T}, 'POINT', 'P') FROM t")),
        ("split_part", format!("SELECT split_part({T}, '(', 1) FROM t")),
        ("starts_with", format!("SELECT starts_with({T}, 'POINT') FROM t")),
        ("trim", format!("SELECT trim({T}) FROM t")),
        ("md5(text)", format!("SELECT md5({T}) FROM t")),
        ("md5(binary)", format!("SELECT md5({B}) FROM t")),
        ("sha256(binary)", format!("SELECT sha256({B}) FROM t")),
        ("encode(binary,'hex')", format!("SELECT encode({B}, 'hex') FROM t")),
        ("encode(binary,'base64')", format!("SELECT encode({B}, 'base64') FROM t")),
        ("to_hex? (n/a) -> octet_length(binary)", format!("SELECT octet_length({B}) FROM t")),
        ("length(binary)", format!("SELECT length({B}) FROM t")),
        ("substr(binary)", format!("SELECT substr({B}, 1, 1) FROM t")),
        ("CAST binary AS BYTEA", format!("SELECT CAST({B} AS BYTEA) FROM t")),
        ("UNION ALL binary with literal", format!("SELECT {B} AS b FROM t UNION ALL SELECT X'00' AS b")),
        ("= text literal", format!("SELECT {T} = 'POINT(1 2)' FROM t")),
        ("= ST_AsText(other col)", "SELECT ST_AsText(wkb4326) = ST_AsText(wkb) FROM t".into()),
        ("< text", format!("SELECT {T} < 'Q' FROM t")),
        ("= binary", "SELECT ST_AsBinary(wkb4326) = ST_AsBinary(wkb) FROM t".into()),
        ("IN (list)", format!("SELECT {T} IN ('POINT(1 2)', 'x') FROM t")),
        ("CASE text/literal", format!("SELECT CASE WHEN id = 1 THEN {T} ELSE 'none' END FROM t")),
        ("COALESCE text/literal", format!("SELECT COALESCE({T}, 'none') FROM t")),
        ("UNION ALL with literal", format!("SELECT {T} AS s FROM t UNION ALL SELECT 'abc' AS s")),
        ("GROUP BY text", format!("SELECT {T} AS s, count(*) FROM t GROUP BY 1 ORDER BY 1")),
        ("GROUP BY binary", format!("SELECT b, count(*) FROM (SELECT {B} AS b FROM t) GROUP BY b ORDER BY 2")),
        ("DISTINCT text", format!("SELECT DISTINCT {T} FROM t ORDER BY 1")),
        ("ORDER BY text", format!("SELECT {T} AS s FROM t ORDER BY s DESC")),
        ("string_agg", format!("SELECT string_agg({T}, ';') FROM t")),
        ("min/max text", format!("SELECT min({T}), max({T}) FROM t")),
        ("array_agg text", format!("SELECT array_agg({T}) FROM t")),
        ("CAST AS VARCHAR", format!("SELECT CAST({T} AS VARCHAR) FROM t")),
        ("arrow_cast Utf8", format!("SELECT arrow_cast({T}, 'Utf8') FROM t")),
        ("arrow_cast LargeUtf8", format!("SELECT arrow_cast({T}, 'LargeUtf8') FROM t")),
        ("CAST binary AS VARCHAR", format!("SELECT CAST({B} AS VARCHAR) FROM t")),
        ("ST_AsText(ST_AsText(g)) (chaining)", format!("SELECT ST_AsText({T}) FROM t")),
        ("ST_GeomFromText(ST_AsText(g))", format!("SELECT ST_AsText(ST_GeomFromText({T})) FROM t")),
        ("ST_AsText('garbage' || ...) into geometry fn", format!("SELECT ST_Area('x' || {T}) FROM t")),
        ("join on text", format!("SELECT count(*) FROM t AS t1 JOIN t AS t2 ON ST_AsText(t1.wkb4326) = ST_AsText(t2.wkb)")),
    ]
}

#[tokio::main]
async fn main() {
    let ctx = context();
    // Control: the same query on untagged values. arrow_cast(x, 'Utf8'/'Binary') drops the
    // field metadata (verified in the first rows of the output).
    let tc = "arrow_cast(ST_AsText(wkb4326), 'Utf8')";
    let bc = "arrow_cast(ST_AsBinary(wkb4326), 'Binary')";
    println!("| query | tagged: result | tagged: output field(s) | untagged control: result | control: output field(s) |");
    println!("|---|---|---|---|---|");
    for (name, sql) in queries() {
        let o = run(&ctx, &sql).await;
        let csql = sql.replace(T, tc).replace(B, bc);
        let c = run(&ctx, &csql).await;
        let f = |o: &e4_type_model::Outcome| if o.ok { format!("ok → {}", o.detail) } else { format!("**FAIL ({})**: {}", o.stage, o.detail) };
        println!("| {name} | {} | {} | {} | {} |", f(&o), o.fields.join(", "), f(&c), c.fields.join(", "));
    }

    // Export: write Parquet with DataFusion, both tagged outputs and derived strings.
    let out = std::env::args().nth(1).unwrap_or_else(|| "h3b_out".into());
    std::fs::create_dir_all(&out).unwrap();
    let path = format!("{out}/astext_asbinary.parquet");
    let sql = format!(
        "COPY (SELECT id, {T} AS wkt, {B} AS wkb, {T} || ' junk' AS wkt_junk, upper({T}) AS wkt_upper, \
         ST_Centroid(wkb4326) AS centroid FROM t) TO '{path}' STORED AS PARQUET"
    );
    let o = run(&ctx, &sql).await;
    println!("\nCOPY TO parquet: ok={} {}", o.ok, o.detail);
    let path3 = format!("{out}/union_literal.parquet");
    let o = run(&ctx, &format!("COPY (SELECT {T} AS s FROM t UNION ALL SELECT 'abc' AS s) TO '{path3}' STORED AS PARQUET")).await;
    println!("COPY union-with-literal TO parquet: ok={} {}", o.ok, o.detail);
    let path4 = format!("{out}/cast_varchar.parquet");
    let o = run(&ctx, &format!("COPY (SELECT CAST({B} AS VARCHAR) AS s FROM t WHERE id < 0) TO '{path4}' STORED AS PARQUET")).await;
    println!("COPY CAST(ST_AsBinary AS VARCHAR) (0 rows) TO parquet: ok={} {}", o.ok, o.detail);

    // The same via DataFrame::write_parquet (uses the logical schema with metadata).
    let df = ctx
        .sql(&format!(
            "SELECT id, {T} AS wkt, {B} AS wkb, {T} || ' junk' AS wkt_junk, upper({T}) AS wkt_upper, \
             ST_Centroid(wkb4326) AS centroid FROM t"
        ))
        .await
        .unwrap();
    for f in df.schema().fields() {
        println!("logical schema: {} -> {}", f.name(), e4_type_model::describe_field(f));
    }
    let path2 = format!("{out}/astext_asbinary_df.parquet");
    let r = df
        .write_parquet(&path2, datafusion::dataframe::DataFrameWriteOptions::new().with_single_file_output(true), None)
        .await;
    println!("DataFrame::write_parquet: {:?}", r.map(|_| ()));

    // Read back with DataFusion.
    let o = run(&ctx, &format!("SELECT * FROM '{path2}'")).await;
    println!("read back (DataFusion) fields: {}", o.fields.join(", "));
    ctx.register_parquet("back", &path2, Default::default()).await.unwrap();
    let o = run(&ctx, "SELECT ST_AsText(wkt_junk) FROM back").await;
    println!("ST_AsText(wkt_junk) after read-back: ok={} {}", o.ok, o.detail);
}
