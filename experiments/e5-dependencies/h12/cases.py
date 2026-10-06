#!/usr/bin/env python3
"""H12 step 1: export every coordinate that a PostGIS doc-test ST_Transform call transforms.

For each ST_Transform (and ST_[Inverse]TransformPipeline) call in the postgis_docs .slt records,
PostGIS computes the call's input geometry (after any inner function such as ST_Centroid or
ST_Intersection) and its output. Both are dumped point by point at full float8 precision into
cases.tsv, which the Rust program (proj-check) reads.

Columns: case, mode, from, to, idx, x, y, z, pg_x, pg_y, pg_z
"""
import os
import sys

import psycopg2

URL = os.environ.get("POSTGIS_URL", "postgresql://postgres:postgres@localhost:54329/postgres")
HERE = os.path.dirname(os.path.abspath(__file__))

L_PT = "'SRID=4326;POINT(-72.1235 42.3521)'::geometry"
L_LN = "'SRID=4326;LINESTRING(-72.1260 42.45, -72.123 42.1546)'::geometry"
L_PTZ = "ST_GeomFromEWKT('SRID=4326;POINT(-72.1235 42.3521 4)')"
L_PTZ2 = "ST_GeomFromEWKT('SRID=4326;POINT(-72.1235 42.3521 10000)')"
L_LNZ = "ST_GeomFromEWKT('SRID=4326;LINESTRING(-72.1260 42.45 15, -72.123 42.1546 20)')"
MA_POLY = ("ST_GeomFromText('POLYGON((743238 2967416,743238 2967450,743265 2967450,"
           "743265.625 2967416,743238 2967416))',2249)")
MA_CIRC = ("ST_GeomFromEWKT('SRID=2249;CIRCULARSTRING(743238 2967416 1,743238 2967450 2,"
           "743265 2967450 3,743265.625 2967416 3,743238 2967416 4)')")
GNOM = "+proj=gnom +ellps=WGS84 +lat_0=70 +lon_0=-160 +no_defs"
P1 = "ST_GeomFromText('POLYGON((170 50,170 72,-130 72,-130 50,170 50))', 4326)"
P2 = "ST_GeomFromText('POLYGON((-170 68,-170 90,-141 90,-141 68,-170 68))', 4326)"
NV_LINE = "ST_GeomFromText('LINESTRING(-118.584 38.374,-118.583 38.5)', 4326)"
NV_PT = "ST_GeomFromText('POINT(-118 38)', 4326)"
PIPE = "urn:ogc:def:coordinateOperation:EPSG::16031"

# id -> (input geometry SQL, mode, from, to, PostGIS output SQL using {inp})
# mode: crs = proj_create_crs_to_crs + normalize_for_visualization (what ST_Transform does);
#       pipe_fwd / pipe_inv = proj_create(pipeline) + normalize, forward / inverse.
def crs(inp, to, frm=None, pg=None):
    return (inp, "crs", frm, to, pg)

CASES = {
    "ma_poly_4326": crs(MA_POLY, "EPSG:4326"),
    "ma_circ_4326": crs(MA_CIRC, "EPSG:4326"),
    "p1_gnom": crs(P1, GNOM, pg=f"ST_Transform({{inp}}, '{GNOM}')"),
    "p2_gnom": crs(P2, GNOM, pg=f"ST_Transform({{inp}}, '{GNOM}')"),
    "isect_gnom_4326": crs(f"ST_Intersection(ST_Transform({P1}, '{GNOM}'), ST_Transform({P2}, '{GNOM}'))",
                           "EPSG:4326", frm=GNOM, pg=f"ST_Transform({{inp}}, '{GNOM}', 4326)"),
    "ptz_2163": crs(L_PTZ, "EPSG:2163"),
    "lnz_2163": crs(L_LNZ, "EPSG:2163"),
    "ptz10000_2163": crs(L_PTZ2, "EPSG:2163"),
    "pt_3857": crs(L_PT, "EPSG:3857"),
    "ln_3857": crs(L_LN, "EPSG:3857"),
    "pt_26986": crs(L_PT, "EPSG:26986"),
    "ln_26986": crs(L_LN, "EPSG:26986"),
    "pt_2163": crs(L_PT, "EPSG:2163"),
    "ln_2163": crs(L_LN, "EPSG:2163"),
    "nv_centroid_32611": crs(f"ST_Centroid({NV_LINE})", "EPSG:32611"),
    "nv_pt_32611": crs(NV_PT, "EPSG:32611"),
    "nv_line_32611": crs(NV_LINE, "EPSG:32611"),
    "boston_4269_26986": crs("ST_SetSRID(ST_Point(-71.063526, 42.35785),4269)", "EPSG:26986"),
    "gda94_4939_7844": crs("'SRID=4939;POINT(143.0 -37.0)'::geometry", "EPSG:7844"),
    "ma_poly_26986": crs(f"ST_SetSRID({MA_POLY}, 2249)", "EPSG:26986"),
    "pa_2273_4326": crs("ST_Point(3637510, 3014852, 2273)", "EPSG:4326"),
    "victoria_4326_3785": crs("ST_SetSRID(ST_Point(-123.365556, 48.428611),4326)", "EPSG:3785"),
    "ln3_4326_26986": crs("ST_GeomFromEWKT('SRID=4326;LINESTRING(-72.1260 42.45, -72.1240 42.45666, -72.123 42.1546)')",
                          "EPSG:26986"),
    "pipe_fwd_16031": ("'SRID=4326;POINT(2 49)'::geometry", "pipe_fwd", PIPE, "",
                       f"ST_TransformPipeline({{inp}}, '{PIPE}')"),
    "pipe_inv_16031": ("'POINT(426857.9877165967 5427937.523342293)'::geometry", "pipe_inv", PIPE, "",
                       f"ST_InverseTransformPipeline({{inp}}, '{PIPE}')"),
}

DUMP = """
SELECT (d).path::text, ST_X((d).geom), ST_Y((d).geom), ST_Z((d).geom)
FROM (SELECT ST_DumpPoints({g}) AS d) s ORDER BY 1
"""


def main():
    conn = psycopg2.connect(URL)
    cur = conn.cursor()
    cur.execute("SET extra_float_digits = 3")
    out = open(os.path.join(HERE, "cases.tsv"), "w")
    out.write("case\tmode\tfrom\tto\tidx\tx\ty\tz\tpg_x\tpg_y\tpg_z\n")
    for cid, (inp, mode, frm, to, pg) in CASES.items():
        if mode == "crs" and frm is None:
            cur.execute(f"SELECT ST_SRID({inp})")
            frm = f"EPSG:{cur.fetchone()[0]}"
        pg_sql = (pg or f"ST_Transform({{inp}}, {to.split(':')[1]})").format(inp=inp)
        cur.execute(DUMP.format(g=inp))
        ins = cur.fetchall()
        cur.execute(DUMP.format(g=pg_sql))
        outs = cur.fetchall()
        assert len(ins) == len(outs), cid
        for (p1, x, y, z), (p2, px, py, pz) in zip(ins, outs):
            assert p1 == p2
            f = lambda v: "" if v is None else repr(v)
            out.write("\t".join([cid, mode, frm, to, p1, f(x), f(y), f(z), f(px), f(py), f(pz)]) + "\n")
    cur.execute("SELECT postgis_full_version()")
    print(cur.fetchone()[0], file=sys.stderr)


if __name__ == "__main__":
    main()
