#!/usr/bin/env python3
"""H12 step 3: re-run every ST_Transform doc-test record in PostGIS with each ST_Transform call
replaced by the geometry PROJ (proj-check) computed, and compare with the recorded expectation.

Usage: records.py <results.tsv>
"""
import decimal
import os
import re
import sys

import psycopg2

from slt_records import records

URL = os.environ.get("POSTGIS_URL", "postgresql://postgres:postgres@localhost:54329/postgres")
HERE = os.path.dirname(os.path.abspath(__file__))

ORIG_NV = "(SELECT ST_GeomFromText('LINESTRING(-118.584 38.374,-118.583 38.5)', 4326) As geom) as foo"

# (file, n) -> rewritten SQL; {case} is replaced by PROJ's output geometry for that case.
REWRITE = {
    ("st_3ddistance.slt", 1): "SELECT ST_3DDistance({ptz_2163},{lnz_2163}), ST_Distance({pt_2163},{ln_2163})",
    ("st_3ddwithin.slt", 1): "SELECT ST_3DDWithin({ptz_2163},{lnz_2163},126.8), ST_DWithin({ptz_2163},{lnz_2163},126.8)",
    ("st_3dmaxdistance.slt", 1): "SELECT ST_3DMaxDistance({ptz10000_2163},{lnz_2163}), ST_MaxDistance({ptz10000_2163},{lnz_2163})",
    ("st_area.slt", 1): "SELECT ST_Area('SRID=2249;POLYGON((743238 2967416,743238 2967450,743265 2967450,743265.625 2967416,743238 2967416))'::geometry), ST_Area({ma_poly_26986})",
    ("st_area.slt", 2): "SELECT ST_Area(geog) / 0.3048 ^ 2, ST_Area(geog, false) / 0.3048 ^ 2, ST_Area(geog) FROM (SELECT {ma_poly_4326}::geography geog) s",
    ("st_buffer.slt", 1): "SELECT ST_AsText(ST_Buffer({boston_4269_26986},100,2))",
    ("st_distance.slt", 1): "SELECT ST_Distance({pt_3857},{ln_3857})",
    ("st_distance.slt", 2): "SELECT ST_Distance({pt_3857},{ln_3857}) * cosd(42.3521)",
    ("st_distance.slt", 3): "SELECT ST_Distance({pt_26986},{ln_26986})",
    ("st_distance.slt", 4): "SELECT ST_Distance({pt_2163},{ln_2163})",
    ("st_distance_spheroid.slt", 1):
        "SELECT round(CAST(ST_DistanceSpheroid(ST_Centroid(geom), ST_GeomFromText('POINT(-118 38)',4326), 'SPHEROID[\"WGS 84\",6378137,298.257223563]') As numeric),2),"
        " round(CAST(ST_DistanceSphere(ST_Centroid(geom), ST_GeomFromText('POINT(-118 38)',4326)) As numeric),2),"
        " round(CAST(ST_Distance({nv_centroid_32611},{nv_pt_32611}) As numeric),2) FROM " + ORIG_NV,
    ("st_distancesphere.slt", 1):
        "SELECT round(CAST(ST_DistanceSphere(ST_Centroid(geom), ST_GeomFromText('POINT(-118 38)',4326)) As numeric),2),"
        " round(CAST(ST_Distance({nv_centroid_32611},{nv_pt_32611}) As numeric),2),"
        " round(CAST(ST_Distance(ST_Centroid(geom), ST_GeomFromText('POINT(-118 38)', 4326)) As numeric),5),"
        " round(CAST(ST_Distance({nv_line_32611},{nv_pt_32611}) As numeric),2) FROM " + ORIG_NV,
    ("st_inversetransformpipeline.slt", 1): "SELECT ST_AsText({pipe_inv_16031})",
    ("st_inversetransformpipeline.slt", 2): "SELECT ST_AsText({gda94_4939_7844})",
    ("st_length.slt", 1): "SELECT ST_Length({ln3_4326_26986})",
    ("st_point.slt", 1): "SELECT ST_AsEWKT({pa_2273_4326}::geography)",
    ("st_setsrid.slt", 1): "SELECT ST_AsEWKT({victoria_4326_3785})",
    ("st_transform.slt", 1): "SELECT ST_AsText({ma_poly_4326})",
    ("st_transform.slt", 2): "SELECT ST_AsEWKT({ma_circ_4326})",
    # The inner ST_Intersection runs on PostGIS's gnomonic geometries (the p1_gnom / p2_gnom cases
    # are compared coordinate by coordinate); PROJ does the final transform back to 4326.
    ("st_transform.slt", 3): "SELECT ST_AsText({isect_gnom_4326})",
    ("st_transformpipeline.slt", 1): "SELECT ST_AsText({pipe_fwd_16031})",
    ("st_transformpipeline.slt", 2): "SELECT ST_AsText({pipe_inv_16031})",
}


# Records whose original output column is geometry/geography (rendered by the harness at 12 digits).
GEOMETRY_OUTPUT = {("st_point.slt", 1), ("st_setsrid.slt", 1)}


def load_geoms(results_path):
    cases = {}
    meta = {}
    for line in open(os.path.join(HERE, "cases.tsv")).read().splitlines()[1:]:
        f = line.split("\t")
        meta[f[0]] = (f[1], f[3])
    for line in open(results_path).read().splitlines()[1:]:
        f = line.split("\t")
        case, idx, sx, sy, sz, pgz = f[0], f[1], f[4], f[5], f[6], f[9]
        cases.setdefault(case, []).append((idx, sx, sy, sz if pgz else None))
    geoms = {}
    for case, pts in cases.items():
        mode, to = meta[case]
        srid = int(to.split(":")[1]) if mode == "crs" and to.startswith("EPSG:") else 0
        has_z = pts[0][3] is not None
        coords = ",".join(" ".join(c for c in p[1:4] if c is not None) for p in pts)
        z = " Z" if has_z else ""
        if pts[0][0].startswith("{1,"):
            wkt = f"POLYGON{z}(({coords}))"
        elif case == "ma_circ_4326":
            wkt = f"CIRCULARSTRING{z}({coords})"
        elif len(pts) == 1:
            wkt = f"POINT{z}({coords})"
        else:
            wkt = f"LINESTRING{z}({coords})"
        geoms[case] = f"'SRID={srid};{wkt}'::geometry"
    return geoms


def harness_float(v):
    r = float(f"{float(v):.11e}")
    return "0" if r == 0 else repr(r).removesuffix(".0") if repr(r).endswith(".0") else repr(r)


def render(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (float, decimal.Decimal)):
        return harness_float(v)
    return str(v)


NUM = re.compile(r"-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?")


def agree12(expected, actual):
    """Numeric tokens equal after rounding both to 12 significant digits; other text equal."""
    ne, na = NUM.findall(expected), NUM.findall(actual)
    if NUM.sub("#", expected) != NUM.sub("#", actual) or len(ne) != len(na):
        return False, float("nan")
    worst = 0.0
    for a, b in zip(ne, na):
        a, b = float(a), float(b)
        if f"{a:.11e}" != f"{b:.11e}":
            return False, abs(a - b) / max(abs(a), abs(b))
        if a != b:
            worst = max(worst, abs(a - b) / max(abs(a), abs(b)))
    return True, worst


def main():
    geoms = load_geoms(sys.argv[1])
    conn = psycopg2.connect(URL)
    cur = conn.cursor()
    cur.execute("SET extra_float_digits = 3")
    recs = records()
    missing = [(f, n) for f, n, _, _ in recs if (f, n) not in REWRITE]
    assert not missing, missing
    ok = 0
    print("| record | expected (slt) | PROJ result | exact | 12 sig. digits |")
    print("|---|---|---|---|---|")
    for f, n, _, expected in recs:
        sql = REWRITE[(f, n)].format(**geoms)
        cur.execute(sql)
        row = cur.fetchone()
        actual = " ".join(render(v) for v in row)
        # ST_AsEWKT output of geometry/geography columns: the harness renders those at 12 digits.
        if (f, n) in GEOMETRY_OUTPUT:
            actual = NUM.sub(lambda m: harness_float(m.group()), actual)
        good, worst = agree12(expected, actual)
        ok += good
        short = lambda s: s if len(s) < 60 else s[:57] + "..."
        print(f"| {f} #{n} | `{short(expected)}` | `{short(actual)}` | {expected == actual} | {good} |")
    print(f"\nrecords agreeing to 12 significant digits: {ok}/{len(recs)}")


if __name__ == "__main__":
    main()
