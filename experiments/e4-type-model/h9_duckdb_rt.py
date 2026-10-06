"""DuckDB round trip of one tagged Parquet file (run in a subprocess: DuckDB can segfault here).
Usage: python h9_duckdb_rt.py <raw.parquet> <out.parquet> <step>"""
import sys, json
import duckdb, pyarrow.parquet as pq
raw, outp, step = sys.argv[1], sys.argv[2], sys.argv[3]
t = pq.read_table(raw)
c = duckdb.connect()
c.sql("LOAD spatial;")
rel = c.from_arrow(t)
if step == "type":
    print(rel.types[1])
elif step == "arrow":
    b = rel.arrow()
    b = b.read_all() if hasattr(b, "read_all") else b
    f = b.schema.field("geometry")
    print({k.decode(): v.decode() for k, v in (f.metadata or {}).items()} or f.type)
elif step == "copy":
    c.sql(f"COPY (SELECT * FROM rel) TO '{outp}' (FORMAT parquet)")
    md = pq.read_schema(outp).metadata or {}
    print(json.dumps(json.loads(md[b"geo"])["columns"]["geometry"].get("crs", "KEY OMITTED")) if b"geo" in md else "no geo metadata")
elif step == "read_raw":
    print(c.sql(f"SELECT geometry FROM read_parquet('{raw}')").types[0])
