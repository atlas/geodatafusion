"""H9 extra: GeoParquet files whose `geo` metadata has crs as a plain string ("EPSG:4326",
"OGC:CRS84") instead of PROJJSON. Do readers accept them?"""
import json, os, subprocess, sys
import pyarrow as pa, pyarrow.parquet as pq, shapely, geopandas as gpd, duckdb
out = sys.argv[1]
con = duckdb.connect(); con.sql("INSTALL spatial; LOAD spatial;")
for crs in ["EPSG:4326", "OGC:CRS84", "EPSG:3857"]:
    geo = {"version": "1.1.0", "primary_column": "geometry",
           "columns": {"geometry": {"encoding": "WKB", "geometry_types": ["Point"], "crs": crs}}}
    t = pa.table({"geometry": pa.array([shapely.to_wkb(shapely.Point(10, 59))])})
    t = t.replace_schema_metadata({"geo": json.dumps(geo)})
    p = f"{out}/geo_string_crs_{crs.replace(':', '_')}.parquet"
    pq.write_table(t, p)
    try:
        c = gpd.read_parquet(p).crs
        g = f"{c.to_string()} (to_epsg={c.to_epsg()})" if c else None
    except Exception as e:
        g = "FAIL " + str(e)[:100]
    try:
        d = con.sql(f"SELECT geometry FROM read_parquet('{p}')").types[0]
    except Exception as e:
        d = "FAIL " + str(e)[:100]
    r = subprocess.run(["ogrinfo", "-so", "-al", p], capture_output=True, text=True)
    ids = [l.strip() for l in r.stdout.splitlines() if l.strip().startswith("ID[")]
    o = ids[-1] if ids else ("(unknown)" if "(unknown)" in r.stdout else r.stderr.strip()[:100])
    print(f"- geo.crs = {crs!r}: geopandas -> {g}; duckdb -> {d}; GDAL -> {o}")
