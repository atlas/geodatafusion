"""H3b export side: what do Python tools make of Parquet files written by DataFusion with
GeoArrow-tagged Utf8/Binary columns (ST_AsText/ST_AsBinary outputs), and of the published
geodatafusion Python package's outputs?

Run: uv run --no-project --with geodatafusion --with datafusion --with pyarrow \
       --with geoarrow-pyarrow --with geopandas --with duckdb python h3b_export.py out/h3b
"""
import sys, glob, os, traceback, json

import pyarrow as pa
import pyarrow.parquet as pq

d = sys.argv[1]


def short(e):
    s = f"{type(e).__name__}: {e}".replace("\n", " ")
    return s[:240]


def fields(schema):
    out = []
    for f in schema:
        md = {k.decode(): v.decode() for k, v in (f.metadata or {}).items()}
        ext = md.get("ARROW:extension:name", "-")
        em = md.get("ARROW:extension:metadata", "")
        out.append(f"{f.name}: {f.type} [{ext} {em}]".replace(" ]", "]"))
    return out


files = sorted(glob.glob(os.path.join(d, "*.parquet")))
print("## pyarrow without geoarrow-pyarrow\n")
for p in files:
    s = pq.read_schema(p)
    print(f"- {os.path.basename(p)}: " + "; ".join(fields(s)))
    print(f"  schema metadata keys: {list((s.metadata or {}).keys())}")
    t = pq.read_table(p)
    try:
        import pyarrow.compute as pc
        if "wkt" in t.column_names:
            print("  pc.utf8_upper(wkt):", pc.utf8_upper(t["wkt"]).to_pylist())
    except Exception as e:
        print("  pc.utf8_upper(wkt) FAIL:", short(e))

print("\n## pyarrow with geoarrow-pyarrow imported (extension types registered)\n")
import geoarrow.pyarrow as ga  # noqa: E402,F401
import pyarrow.compute as pc  # noqa: E402

for p in files:
    try:
        t = pq.read_table(p)
        print(f"- {os.path.basename(p)}: read ok; types: " + "; ".join(f"{f.name}: {f.type}" for f in t.schema))
    except Exception as e:
        print(f"- {os.path.basename(p)}: read FAIL: {short(e)}")
        continue
    for name in t.column_names:
        col = t[name]
        if isinstance(col.type, pa.ExtensionType):
            try:
                print(f"  {name}: ga.as_wkt -> {ga.as_wkt(col).to_pylist()[:3]}")
            except Exception as e:
                print(f"  {name}: ga.as_wkt FAIL: {short(e)}")
            if pa.types.is_string(col.type.storage_type) or pa.types.is_large_string(col.type.storage_type):
                for fn in ["utf8_upper", "utf8_length"]:
                    try:
                        r = getattr(pc, fn)(col)
                        print(f"  {name}: pc.{fn} ok -> {r.to_pylist()[:2]}")
                    except Exception as e:
                        print(f"  {name}: pc.{fn} FAIL: {short(e)}")
                try:
                    print(f"  {name}: to_pandas dtype -> {t.select([name]).to_pandas()[name].dtype}")
                except Exception as e:
                    print(f"  {name}: to_pandas FAIL: {short(e)}")

print("\n## geopandas\n")
import geopandas as gpd  # noqa: E402

for p in files:
    try:
        g = gpd.read_parquet(p)
        print(f"- read_parquet {os.path.basename(p)}: ok, geometry columns: "
              f"{[c for c in g.columns if str(g[c].dtype) == 'geometry']}, crs={g.crs.to_string() if g.crs else None}")
    except Exception as e:
        print(f"- read_parquet {os.path.basename(p)}: FAIL: {short(e)}")
    try:
        t = pq.read_table(p)
        g = gpd.GeoDataFrame.from_arrow(t)
        print(f"  from_arrow: ok, geometry={g.geometry.name}, crs={g.crs.to_string() if g.crs else None}, "
              f"values={[x.wkt if x is not None else None for x in g.geometry][:3]}")
    except Exception as e:
        print(f"  from_arrow: FAIL: {short(e)}")

print("\n## DuckDB\n")
import duckdb  # noqa: E402

con = duckdb.connect()
try:
    con.sql("INSTALL spatial; LOAD spatial;")
    spatial = True
except Exception as e:
    print("spatial extension unavailable:", short(e))
    spatial = False
for p in files:
    try:
        r = con.sql(f"DESCRIBE SELECT * FROM read_parquet('{p}')").fetchall()
        print(f"- {os.path.basename(p)}: " + "; ".join(f"{c[0]}: {c[1]}" for c in r))
        rows = con.sql(f"SELECT * FROM read_parquet('{p}')").fetchall()
        print(f"  rows: {str(rows)[:200]}")
    except Exception as e:
        print(f"- {os.path.basename(p)}: FAIL: {short(e)}")
    try:
        t = pq.read_table(p)
        r = con.sql("DESCRIBE SELECT * FROM t").fetchall()
        print(f"  from arrow table: " + "; ".join(f"{c[0]}: {c[1]}" for c in r))
        if spatial and any("GEOMETRY" in c[1] for c in r):
            for c in r:
                if "GEOMETRY" in c[1]:
                    print(f"  ST_AsText({c[0]}): {con.sql(f'SELECT ST_AsText({c[0]}) FROM t').fetchall()}")
    except Exception as e:
        print(f"  from arrow table: FAIL: {short(e)}")

print("\n## Published Python packages (geodatafusion + datafusion)\n")
try:
    import datafusion
    import geodatafusion
    ctx = datafusion.SessionContext()
    geodatafusion.register_all(ctx)
    for sql in [
        "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) AS t",
        "SELECT ST_AsBinary(ST_GeomFromText('POINT(1 2)')) AS b",
        "SELECT upper(ST_AsText(ST_GeomFromText('POINT(1 2)'))) AS t",
        "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) || 'x' AS t",
        "SELECT md5(ST_AsBinary(ST_GeomFromText('POINT(1 2)'))) AS t",
        "SELECT encode(ST_AsBinary(ST_GeomFromText('POINT(1 2)')), 'hex') AS t",
        "SELECT ST_AsText(ST_GeomFromText('POINT(1 2)')) LIKE 'POINT%' AS t",
    ]:
        try:
            tbl = ctx.sql(sql).to_arrow_table()
            f = tbl.schema.field(0)
            print(f"- `{sql}`: ok, type={f.type}, value={tbl.column(0).to_pylist()}, field metadata={f.metadata}")
            try:
                print(f"    to_pandas -> {tbl.to_pandas().iloc[0, 0]!r}")
            except Exception as e:
                print(f"    to_pandas FAIL: {short(e)}")
        except Exception as e:
            print(f"- `{sql}`: FAIL: {short(e)}")
except Exception as e:
    print("published packages FAIL:", short(e))
    traceback.print_exc()

print("\n## Controls\n")
t = pq.read_table(os.path.join(d, "astext_asbinary.parquet"))
try:
    g = gpd.GeoDataFrame.from_arrow(t.drop_columns(["wkt"]))
    print(f"- from_arrow without the geoarrow.wkt column: ok, geometry={g.geometry.name}, crs={g.crs.to_string() if g.crs else None}")
except Exception as e:
    print(f"- from_arrow without the geoarrow.wkt column: FAIL: {short(e)}")
try:
    s = t.select(["wkt"]).to_pandas()["wkt"]
    print(f"- pandas .str.upper() on geoarrow.wkt column: {s.str.upper().tolist()}")
except Exception as e:
    print(f"- pandas .str.upper() on geoarrow.wkt column: FAIL: {short(e)}")
try:
    s = t.select(["wkt_upper"]).to_pandas()["wkt_upper"]
    print(f"- pandas .str.upper() on plain string column: {s.str.upper().tolist()}")
except Exception as e:
    print(f"- pandas .str.upper() on plain string column: FAIL: {short(e)}")
