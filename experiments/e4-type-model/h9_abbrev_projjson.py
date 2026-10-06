"""H9 extra: is an abbreviated PROJJSON ({type, name, id} only, what a PROJ-less writer could emit
from an SRID) accepted by GeoPandas, DuckDB and GDAL, as a GeoArrow tag and as GeoParquet crs?"""
import json, subprocess, sys
import pyarrow as pa, pyarrow.parquet as pq, pyarrow.feather as feather, shapely, geopandas as gpd, duckdb
out = sys.argv[1]
ABBR = {
    "4326": {"$schema": "https://proj.org/schemas/v0.7/projjson.schema.json", "type": "GeographicCRS",
             "name": "WGS 84", "id": {"authority": "EPSG", "code": 4326}},
    "3857": {"$schema": "https://proj.org/schemas/v0.7/projjson.schema.json", "type": "ProjectedCRS",
             "name": "WGS 84 / Pseudo-Mercator", "id": {"authority": "EPSG", "code": 3857}},
}
con = duckdb.connect(); con.sql("INSTALL spatial; LOAD spatial;")
wkb = shapely.to_wkb(shapely.Point(10, 59))
for code, pj in ABBR.items():
    f = pa.field("geometry", pa.binary(), metadata={"ARROW:extension:name": "geoarrow.wkb",
        "ARROW:extension:metadata": json.dumps({"crs": pj, "crs_type": "projjson"})})
    t = pa.Table.from_arrays([pa.array([wkb])], schema=pa.schema([f]))
    try:
        c = gpd.GeoDataFrame.from_arrow(t).crs
        g = f"{c.to_string()} (to_epsg={c.to_epsg()})" if c else None
    except Exception as e:
        g = "FAIL " + str(e)[:120]
    r = subprocess.run([sys.executable, "-c", f"""
import duckdb, pyarrow.feather as f
c = duckdb.connect(); c.sql('LOAD spatial'); t = f.read_table('{out}/abbr_{code}.arrow')
print(c.from_arrow(t).types[0])"""], capture_output=True, text=True) if feather.write_feather(t, f"{out}/abbr_{code}.arrow", compression="uncompressed") is None else None
    d = r.stdout.strip() or ("FAIL " + r.stderr.strip()[-120:])
    o = subprocess.run(["ogrinfo", "-so", "-al", f"{out}/abbr_{code}.arrow"], capture_output=True, text=True).stdout
    ids = [l.strip() for l in o.splitlines() if l.strip().startswith("ID[")]
    print(f"- GeoArrow tag, abbreviated PROJJSON EPSG:{code}: geopandas -> {g}; duckdb -> {d}; GDAL IPC -> {ids[-1] if ids else '(unknown)'}")
    geo = {"version": "1.1.0", "primary_column": "geometry",
           "columns": {"geometry": {"encoding": "WKB", "geometry_types": ["Point"], "crs": pj}}}
    p = f"{out}/abbr_geo_{code}.parquet"
    pq.write_table(pa.table({"geometry": pa.array([wkb])}).replace_schema_metadata({"geo": json.dumps(geo)}), p)
    try:
        c = gpd.read_parquet(p).crs
        g = f"{c.to_string()} (to_epsg={c.to_epsg()})" if c else None
    except Exception as e:
        g = "FAIL " + str(e)[:120]
    try:
        d = con.sql(f"SELECT geometry FROM read_parquet('{p}')").types[0]
    except Exception as e:
        d = "FAIL " + str(e)[:120]
    o = subprocess.run(["ogrinfo", "-so", "-al", p], capture_output=True, text=True).stdout
    ids = [l.strip() for l in o.splitlines() if l.strip().startswith("ID[")]
    print(f"- GeoParquet crs, abbreviated PROJJSON EPSG:{code}: geopandas -> {g}; duckdb -> {d}; GDAL -> {ids[-1] if ids else '(unknown)'}")
