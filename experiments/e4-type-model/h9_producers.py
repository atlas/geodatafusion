"""H9: which CRS representation do real producers write for WGS 84 and projected CRSs, and does a
GeoArrow column tagged EPSG:4326 / OGC:CRS84 / PROJJSON round-trip through GeoPandas, DuckDB and
GDAL with its CRS intact?

Run: uv run --no-project --with pyarrow --with geoarrow-pyarrow --with geopandas --with pyogrio \
       --with duckdb --with pyproj python h9_producers.py out/h9
Needs a system GDAL with the Parquet and Arrow drivers (ogr2ogr) for the GDAL rows.
"""
import json, os, subprocess, sys

import pyarrow as pa
import pyarrow.parquet as pq
import pyarrow.feather as feather
import shapely
import geopandas as gpd
import pyproj
import duckdb

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
CRSS = ["EPSG:4326", "OGC:CRS84", "EPSG:3857", "EPSG:2263"]


def summarize_crs(v):
    """Short description of a CRS value as found in metadata."""
    if v is None:
        return "None"
    if isinstance(v, str):
        try:
            v2 = json.loads(v)
            if isinstance(v2, dict):
                return "PROJJSON(str) " + summarize_crs(v2)
        except Exception:
            pass
        return f"string {v[:60]!r}"
    if isinstance(v, dict):
        i = v.get("id") or {}
        return f"PROJJSON name={v.get('name')!r} id={i.get('authority')}:{i.get('code')}"
    return repr(v)[:80]


def geo_meta(path):
    md = pq.read_schema(path).metadata or {}
    if b"geo" not in md:
        return "no geo metadata"
    g = json.loads(md[b"geo"])
    col = g["primary_column"]
    c = g["columns"][col]
    return f"geo v{g.get('version')} encoding={c.get('encoding')} crs " + (
        summarize_crs(c["crs"]) if "crs" in c else "KEY OMITTED (= OGC:CRS84 by spec)")


def field_crs(schema, name=None):
    res = []
    for f in schema:
        md = {k.decode(): v.decode() for k, v in (f.metadata or {}).items()}
        if isinstance(f.type, pa.ExtensionType):  # geoarrow-pyarrow registered: metadata lives in the type
            md["ARROW:extension:name"] = f.type.extension_name
            md["ARROW:extension:metadata"] = f.type.__arrow_ext_serialize__().decode()
        if "ARROW:extension:name" in md and (name is None or f.name == name):
            em = md.get("ARROW:extension:metadata", "")
            try:
                emj = json.loads(em) if em else {}
            except Exception:
                emj = {"raw": em}
            res.append(f"{f.name}: {md['ARROW:extension:name']} crs_type={emj.get('crs_type')} crs={summarize_crs(emj.get('crs'))}")
    return "; ".join(res) or "no extension field"


def section(t):
    print(f"\n## {t}\n")


def short(e):
    return f"{type(e).__name__}: {e}".replace("\n", " ")[:200]


# ---------------------------------------------------------------- producers
section("GeoPandas (to_parquet / to_arrow)")
for crs in CRSS:
    gdf = gpd.GeoDataFrame({"id": [1]}, geometry=[shapely.Point(10, 59)], crs=crs)
    for enc in ["WKB", "geoarrow"]:
        p = f"{out}/geopandas_{crs.replace(':', '_')}_{enc}.parquet"
        gdf.to_parquet(p, geometry_encoding=enc)
        print(f"- to_parquet {crs} encoding={enc}: {geo_meta(p)}")
    for enc in ["WKB", "geoarrow"]:
        t = pa.table(gdf.to_arrow(geometry_encoding=enc))
        print(f"- to_arrow {crs} encoding={enc}: {field_crs(t.schema)}")

section("GDAL ogr2ogr (system GDAL)")
print(subprocess.run(["ogr2ogr", "--version"], capture_output=True, text=True).stdout.strip())
for crs in CRSS:
    src = f"{out}/src_{crs.replace(':', '_')}.geojson"
    # GeoJSON (RFC 7946) is always CRS84, so write the source as a GeoPackage instead.
    src = src.replace(".geojson", ".gpkg")
    gpd.GeoDataFrame({"id": [1]}, geometry=[shapely.Point(10, 59)], crs=crs).to_file(src, engine="pyogrio")
    for enc in ["WKB", "GEOARROW"]:
        p = f"{out}/gdal_{crs.replace(':', '_')}_{enc}.parquet"
        r = subprocess.run(["ogr2ogr", "-f", "Parquet", p, src, "-lco", f"GEOMETRY_ENCODING={enc}"],
                           capture_output=True, text=True)
        if r.returncode:
            print(f"- Parquet {crs} {enc}: FAIL {r.stderr.strip()[:200]}")
            continue
        print(f"- Parquet {crs} encoding={enc}: {geo_meta(p)}; arrow field: {field_crs(pq.read_schema(p))}")
    p = f"{out}/gdal_{crs.replace(':', '_')}.arrow"
    r = subprocess.run(["ogr2ogr", "-f", "Arrow", p, src], capture_output=True, text=True)
    if r.returncode:
        print(f"- Arrow IPC {crs}: FAIL {r.stderr.strip()[:200]}")
    else:
        t = feather.read_table(p)
        md = t.schema.metadata or {}
        gc = json.loads(md[b"geo"])["columns"] if b"geo" in md else {}
        gcrs = next(iter(gc.values()), {}).get("crs") if gc else None
        print(f"- Arrow IPC {crs}: field: {field_crs(t.schema)}; schema-level geo metadata crs: "
              f"{('WKT2 string ' + repr(gcrs[:40]) + '... ID ' + gcrs[gcrs.rfind('ID['):].strip()) if isinstance(gcrs, str) else summarize_crs(gcrs)}")

section("pyogrio read_arrow (GDAL ArrowStream export, bundled GDAL)")
import pyogrio
print("pyogrio", pyogrio.__version__, "GDAL", pyogrio.__gdal_version_string__)
for crs in CRSS:
    src = f"{out}/src_{crs.replace(':', '_')}.gpkg"
    meta, t = pyogrio.read_arrow(src)
    print(f"- read_arrow {crs}: meta crs={meta.get('crs')!r}; field: {field_crs(t.schema)}")

section("DuckDB spatial")
con = duckdb.connect()
con.sql("INSTALL spatial; LOAD spatial;")
print("duckdb", duckdb.__version__)
for crs in CRSS:
    for sql in [f"SELECT ST_SetCRS(ST_Point(10, 59), '{crs}') AS geometry",
                f"SELECT ST_Point(10, 59)::GEOMETRY('{crs}') AS geometry"]:
        try:
            rel = con.sql(sql)
            typ = rel.types[0]
            p = f"{out}/duckdb_{crs.replace(':', '_')}.parquet"
            con.sql(f"COPY ({sql}) TO '{p}' (FORMAT parquet)")
            t = rel.arrow()
            t = t.read_all() if hasattr(t, "read_all") else t
            print(f"- `{sql}`: type {typ}; COPY parquet: {geo_meta(p)}; arrow field: {field_crs(pq.read_schema(p))}; "
                  f".arrow(): {field_crs(t.schema)}")
            break
        except Exception as e:
            print(f"- `{sql}`: FAIL {short(e)}")
# A file DuckDB *reads* with CRS: what does it think the CRS is?
for p in [f"{out}/geopandas_EPSG_4326_WKB.parquet", f"{out}/geopandas_OGC_CRS84_WKB.parquet",
          f"{out}/geopandas_EPSG_3857_WKB.parquet"]:
    try:
        print(f"- DuckDB reads {os.path.basename(p)}: {con.sql(f'DESCRIBE SELECT geometry FROM read_parquet(\'{p}\')').fetchall()[0][1]}")
    except Exception as e:
        print(f"- DuckDB reads {os.path.basename(p)}: FAIL {short(e)}")

section("geoarrow-pyarrow")
import geoarrow.pyarrow as ga
for crs in CRSS:
    a = ga.with_crs(ga.as_wkb(["POINT (10 59)"]), crs)
    a2 = ga.with_crs(ga.as_wkb(["POINT (10 59)"]), pyproj.CRS(crs))
    print(f"- with_crs(str {crs!r}): {a.type.__arrow_ext_serialize__().decode()[:120]}")
    print(f"- with_crs(pyproj.CRS({crs!r})): {summarize_crs(json.loads(a2.type.__arrow_ext_serialize__())['crs'])} "
          f"crs_type={json.loads(a2.type.__arrow_ext_serialize__()).get('crs_type')}")

# ---------------------------------------------------------------- round trips
section("Round trips of a geoarrow.wkb column with a given CRS tag")
projjson_4326 = pyproj.CRS("EPSG:4326").to_json_dict()
TAGS = {
    "authority_code EPSG:4326": {"crs": "EPSG:4326", "crs_type": "authority_code"},
    "authority_code OGC:CRS84": {"crs": "OGC:CRS84", "crs_type": "authority_code"},
    "projjson EPSG:4326": {"crs": projjson_4326, "crs_type": "projjson"},
    "srid 4326": {"crs": "4326", "crs_type": "srid"},
    "no crs_type, EPSG:4326": {"crs": "EPSG:4326"},
}


def tagged_table(meta):
    wkb = shapely.to_wkb(shapely.Point(10, 59))
    f = pa.field("geometry", pa.binary(), metadata={
        "ARROW:extension:name": "geoarrow.wkb",
        "ARROW:extension:metadata": json.dumps(meta)})
    # Build without geoarrow-pyarrow's registered types: plain storage + field metadata.
    return pa.Table.from_arrays([pa.array([1]), pa.array([wkb], pa.binary())],
                                schema=pa.schema([pa.field("id", pa.int64()), f]))


def identifies_4326(desc):
    d = str(desc)
    return any(s in d for s in ["EPSG:4326", "4326", "CRS84", "WGS 84"])


rows = []
for name, meta in TAGS.items():
    t = tagged_table(meta)
    raw = f"{out}/tag_{name.replace(' ', '_').replace(':', '_').replace(',', '')}.parquet"
    pq.write_table(t, raw)  # plain Parquet with the Arrow schema (what DataFusion COPY writes)
    res = {"tag": name}
    # GeoPandas
    try:
        g = gpd.GeoDataFrame.from_arrow(t)
        res["gpd.from_arrow crs"] = g.crs.to_string() if g.crs else None
        back = pa.table(g.to_arrow(geometry_encoding="WKB"))
        res["gpd.to_arrow back"] = field_crs(back.schema)
        p = f"{out}/rt_gpd_{len(rows)}.parquet"
        g.to_parquet(p)
        res["gpd.to_parquet geo"] = geo_meta(p)
        c2 = gpd.read_parquet(p).crs
        res["gpd.read_parquet crs"] = f"{c2.to_string()} (to_epsg={c2.to_epsg()})" if c2 else None
    except Exception as e:
        res["gpd"] = "FAIL " + short(e)
    try:
        g = gpd.read_parquet(raw)
        res["gpd.read_parquet(raw, no geo md)"] = str(g.crs)
    except Exception as e:
        res["gpd.read_parquet(raw, no geo md)"] = "FAIL " + short(e)
    # DuckDB (each step in a subprocess, because DuckDB segfaults on some of them)
    p = f"{out}/rt_duckdb_{len(rows)}.parquet"
    for step, key in [("type", "duckdb.from_arrow type"), ("arrow", "duckdb .arrow() back"),
                      ("copy", "duckdb COPY parquet geo crs"), ("read_raw", "duckdb read_parquet(raw) type")]:
        r = subprocess.run([sys.executable, os.path.join(os.path.dirname(__file__), "h9_duckdb_rt.py"), raw, p, step],
                           capture_output=True, text=True)
        v = r.stdout.strip() if r.returncode == 0 else f"FAIL rc={r.returncode} {r.stderr.strip()[-150:]}"
        if step == "copy" and r.returncode == 0:
            v = summarize_crs(json.loads(v)) if not v.startswith(("\"KEY", "no geo")) else v
        res[key] = v[:300]
    # GDAL: read the raw Parquet file (Arrow schema metadata only, no geo key)
    def ogr_srs(path):
        r = subprocess.run(["ogrinfo", "-so", "-al", path], capture_output=True, text=True)
        txt = r.stdout
        if "Layer SRS WKT:\n(unknown)" in txt:
            return "geometry recognised, SRS (unknown)" if "Geometry Column" in txt or "Geometry:" in txt else "no geometry"
        ids = [l.strip() for l in txt.splitlines() if l.strip().startswith("ID[")]
        return f"SRS {ids[-1]}" if ids else ("no SRS line; " + r.stderr.strip()[:100])
    res["ogrinfo raw parquet"] = ogr_srs(raw)
    ipc = raw.replace(".parquet", ".arrow")
    feather.write_feather(t, ipc, compression="uncompressed")
    res["ogrinfo arrow ipc"] = ogr_srs(ipc)
    gp = raw.replace(".parquet", "_gdal.parquet")
    r = subprocess.run(["ogr2ogr", "-f", "Parquet", gp, ipc], capture_output=True, text=True)
    res["ogr2ogr ipc->parquet geo"] = geo_meta(gp) if r.returncode == 0 else "FAIL " + r.stderr.strip()[:150]
    rows.append(res)

for r in rows:
    print(f"### {r['tag']}\n")
    for k, v in r.items():
        if k != "tag":
            print(f"- {k}: {v}")
    print()
