"""Build the E2 geometry corpus in PostGIS (schema `e2`) and export it.

Reproducible: Python's `random.Random(SEED)` for synthetic geometries, PostgreSQL `setseed()`
and explicit seeds for anything generated in SQL, and stable ids (insertion order).

Usage:
    uv run --with 'psycopg[binary]' --with pyarrow python gen_corpus.py

Writes to $E2_DATA (default: <repo>/target/experiments/e2):
    unary.tsv   id  src  wkb_hex  simplify_tol  vw_tol
    pairs.tsv   id  src  wkb_hex_a  wkb_hex_b
"""

import json
import math
import os
import random
import sys
from pathlib import Path

import psycopg
import pyarrow.parquet as pq

SEED = 20261005
REPO = Path(__file__).resolve().parents[2]
DATA = Path(os.environ.get("E2_DATA", REPO / "target/experiments/e2"))
DSN = os.environ.get("E2_DSN", "postgresql://postgres:postgres@localhost:54330/postgres")
NE = DATA / "ne"
NE_LAYERS = [
    "ne_110m_admin_0_countries",
    "ne_50m_admin_0_countries",
    "ne_50m_admin_1_states_provinces",
    "ne_50m_lakes",
    "ne_50m_rivers_lake_centerlines",
    "ne_110m_populated_places",
    "ne_110m_coastline",
]

rng = random.Random(SEED)


# ---------------------------------------------------------------------------------------------
# Synthetic WKT generators
# ---------------------------------------------------------------------------------------------


def fmt(v):
    return repr(float(v)) if not float(v).is_integer() else str(int(v))


def scale():
    """A coordinate frame: (origin x, origin y, size, integer grid?)."""
    r = rng.random()
    if r < 0.25:
        return (0.0, 0.0, 10.0, True)  # small integer grid: exact touches/collinearity
    if r < 0.6:
        return (rng.uniform(-100, 100), rng.uniform(-100, 100), rng.uniform(1, 100), False)
    if r < 0.8:
        return (1e6 + rng.uniform(0, 1e5), 5e6 + rng.uniform(0, 1e5), rng.uniform(1, 1000), False)
    if r < 0.9:
        return (rng.uniform(-1, 1) * 1e-3, rng.uniform(-1, 1) * 1e-3, 1e-6, False)
    return (rng.uniform(-180, 180), rng.uniform(-80, 80), rng.uniform(0.01, 5), False)


def pt(frame, x, y):
    ox, oy, s, grid = frame
    if grid:
        return (round(ox + x * s), round(oy + y * s))
    return (ox + x * s, oy + y * s)


def coords_str(cs):
    return ",".join(f"{fmt(x)} {fmt(y)}" for x, y in cs)


def ring_str(cs):
    return "(" + coords_str(cs) + ")"


def close(cs):
    return cs + [cs[0]]


def star(frame, n, cx=0.5, cy=0.5, rmin=0.2, rmax=0.5):
    angs = sorted(rng.uniform(0, 2 * math.pi) for _ in range(n))
    cs = []
    for a in angs:
        r = rng.uniform(rmin, rmax)
        cs.append(pt(frame, cx + r * math.cos(a), cy + r * math.sin(a)))
    # dedupe consecutive (grid snapping)
    out = [c for i, c in enumerate(cs) if i == 0 or c != cs[i - 1]]
    if len(out) > 1 and out[0] == out[-1]:
        out.pop()
    return out


def orient_and_rotate(cs):
    """Random orientation and start vertex for an open ring."""
    if rng.random() < 0.5:
        cs = cs[::-1]
    k = rng.randrange(len(cs)) if cs else 0
    return cs[k:] + cs[:k]


def poly_valid_star(frame):
    while True:
        cs = star(frame, rng.randint(3, 80))
        if len(cs) >= 3:
            return f"POLYGON({ring_str(close(orient_and_rotate(cs)))})"


def poly_with_holes(frame):
    shell = star(frame, rng.randint(6, 60), rmin=0.35, rmax=0.5)
    holes = []
    nh = rng.randint(1, 3)
    for k in range(nh):
        a = 2 * math.pi * k / nh
        h = star(frame, rng.randint(3, 12), cx=0.5 + 0.17 * math.cos(a), cy=0.5 + 0.17 * math.sin(a), rmin=0.02, rmax=0.08)
        if len(h) >= 3:
            holes.append(ring_str(close(orient_and_rotate(h))))
    rings = [ring_str(close(orient_and_rotate(shell)))] + holes
    return f"POLYGON({','.join(rings)})"


def poly_random_order(frame):
    n = rng.randint(4, 12)
    cs = [pt(frame, rng.random(), rng.random()) for _ in range(n)]
    cs = [c for i, c in enumerate(cs) if i == 0 or c != cs[i - 1]]
    while len(cs) < 3:
        cs.append(pt(frame, rng.random(), rng.random()))
    if cs[0] == cs[-1]:
        cs.pop()
    if len(cs) < 3:
        cs = [pt(frame, 0, 0), pt(frame, 1, 1), pt(frame, 1, 0), pt(frame, 0, 1)]
    return f"POLYGON({ring_str(close(cs))})"


def poly_special(frame):
    """Hand-shaped invalid or degenerate polygons."""
    kind = rng.randrange(8)
    P = lambda x, y: pt(frame, x, y)  # noqa: E731
    if kind == 0:  # bowtie
        cs = [P(0, 0), P(1, 1), P(1, 0), P(0, 1)]
    elif kind == 1:  # zero-area (collinear) ring
        t = sorted(rng.random() for _ in range(rng.randint(2, 5)))
        cs = [P(0, 0)] + [P(x, x) for x in t] + [P(1, 1)]
        cs = cs + cs[-2:0:-1]
        cs = cs[: max(3, len(cs))]
    elif kind == 2:  # spike
        cs = [P(0, 0), P(1, 0), P(1, 1), P(0.5, 1), P(0.5, 2), P(0.5, 1), P(0, 1)]
    elif kind == 3:  # hole outside shell
        return f"POLYGON({ring_str(close([P(0,0),P(1,0),P(1,1),P(0,1)]))},{ring_str(close([P(2,2),P(3,2),P(3,3)]))})"
    elif kind == 4:  # hole crossing shell
        return f"POLYGON({ring_str(close([P(0,0),P(1,0),P(1,1),P(0,1)]))},{ring_str(close([P(0.5,0.5),P(1.5,0.5),P(1.5,0.7),P(0.5,0.7)]))})"
    elif kind == 5:  # self-touching ring (inverted hole)
        cs = [P(0, 0), P(2, 0), P(2, 2), P(1, 2), P(1.5, 1), P(1, 0.5), P(0.5, 1), P(1, 2), P(0, 2)]
    elif kind == 6:  # repeated vertices
        base = star(frame, rng.randint(3, 10))
        cs = []
        for c in base:
            cs += [c] * rng.randint(1, 3)
    else:  # hole touching shell at a point (valid)
        return f"POLYGON({ring_str(close([P(0,0),P(2,0),P(2,2),P(0,2)]))},{ring_str(close([P(0,0),P(1,0.5),P(0.5,1)]))})"
    cs = [c for i, c in enumerate(cs) if i == 0 or c != cs[i - 1]] if kind != 6 else cs
    if cs[0] == cs[-1]:
        cs = cs[:-1]
    while len(cs) < 3:
        cs.append(cs[-1])
    return f"POLYGON({ring_str(close(orient_and_rotate(cs)))})"


def rect(frame):
    x0, y0 = rng.random() * 0.5, rng.random() * 0.5
    x1, y1 = x0 + rng.random() * 0.5, y0 + rng.random() * 0.5
    cs = [pt(frame, x0, y0), pt(frame, x1, y0), pt(frame, x1, y1), pt(frame, x0, y1)]
    if len(set(cs)) < 4:
        cs = [pt(frame, 0, 0), pt(frame, 1, 0), pt(frame, 1, 1), pt(frame, 0, 1)]
    return f"POLYGON({ring_str(close(orient_and_rotate(cs)))})"


def line_walk(frame, n=None):
    n = n or rng.randint(2, 150)
    x, y = rng.random(), rng.random()
    cs = [pt(frame, x, y)]
    step = rng.choice([0.02, 0.1, 0.3])
    while len(cs) < n:
        x += rng.gauss(0, step)
        y += rng.gauss(0, step)
        c = pt(frame, x, y)
        if c != cs[-1]:
            cs.append(c)
    return cs


def line_special(frame):
    kind = rng.randrange(6)
    if kind == 0:  # zero length
        c = pt(frame, rng.random(), rng.random())
        cs = [c, c]
    elif kind == 1:  # collinear, unordered
        t = [rng.random() for _ in range(rng.randint(2, 8))]
        cs = [pt(frame, v, 2 * v) for v in t]
    elif kind == 2:  # repeated consecutive points
        cs = []
        for c in line_walk(frame, rng.randint(2, 10)):
            cs += [c] * rng.randint(1, 3)
    elif kind == 3:  # closed ring
        s = star(frame, rng.randint(3, 20))
        cs = close(s) if len(s) >= 3 else line_walk(frame, 3)
    elif kind == 4:  # zig-zag self-intersection
        cs = [pt(frame, rng.random(), rng.random()) for _ in range(rng.randint(4, 12))]
    else:  # two points
        cs = line_walk(frame, 2)
    if len(cs) < 2:
        cs = cs + cs
    return cs


def multipoint(frame):
    kind = rng.randrange(5)
    n = rng.randint(1, 50)
    if kind == 0:
        cs = [pt(frame, rng.random(), rng.random()) for _ in range(n)]
    elif kind == 1:  # duplicates
        base = [pt(frame, rng.random(), rng.random()) for _ in range(max(1, n // 3))]
        cs = [rng.choice(base) for _ in range(n)]
    elif kind == 2:  # collinear
        cs = [pt(frame, v, 0.3 * v + 0.1) for v in (rng.random() for _ in range(n))]
    elif kind == 3:  # all identical
        c = pt(frame, rng.random(), rng.random())
        cs = [c] * n
    else:  # cocircular-ish / rectangle corners with interior points
        cs = [pt(frame, 0, 0), pt(frame, 1, 0), pt(frame, 1, 1), pt(frame, 0, 1)]
        cs += [pt(frame, rng.random(), rng.random()) for _ in range(n)]
    rng.shuffle(cs)
    return "MULTIPOINT(" + ",".join(f"({fmt(x)} {fmt(y)})" for x, y in cs) + ")"


def synthetic():
    out = []

    def add(src, wkt, count):
        for _ in range(count):
            out.append((src, wkt() if callable(wkt) else wkt))

    add("syn:point", lambda: (lambda f: "POINT({} {})".format(*map(fmt, pt(f, rng.random(), rng.random()))))(scale()), 300)
    add("syn:multipoint", lambda: multipoint(scale()), 500)
    add("syn:line_walk", lambda: f"LINESTRING({coords_str(line_walk(scale()))})", 600)
    add("syn:line_special", lambda: f"LINESTRING({coords_str(line_special(scale()))})", 500)
    add("syn:multiline", lambda: (lambda f: "MULTILINESTRING(" + ",".join(
        "(" + coords_str(line_walk(f, rng.randint(2, 30)) if rng.random() < 0.8 else line_special(f)) + ")"
        for _ in range(rng.randint(1, 6))) + ")")(scale()), 300)
    add("syn:poly_star", lambda: poly_valid_star(scale()), 700)
    add("syn:poly_holes", lambda: poly_with_holes(scale()), 300)
    add("syn:poly_rect", lambda: rect(scale()), 200)
    add("syn:poly_random_order", lambda: poly_random_order(scale()), 300)
    add("syn:poly_special", lambda: poly_special(scale()), 400)

    def mpoly_disjoint():
        f = scale()
        parts = []
        for k in range(rng.randint(1, 5)):
            g = (f[0] + k * 1.2 * f[2], f[1], f[2], f[3])
            s = star(g, rng.randint(3, 30))
            if len(s) >= 3:
                parts.append("(" + ring_str(close(orient_and_rotate(s))) + ")")
        return "MULTIPOLYGON(" + ",".join(parts) + ")" if parts else "MULTIPOLYGON EMPTY"

    def mpoly_overlap():
        f = scale()
        parts = []
        for _ in range(rng.randint(2, 4)):
            s = star(f, rng.randint(3, 20), cx=rng.uniform(0.3, 0.7), cy=rng.uniform(0.3, 0.7))
            if len(s) >= 3:
                parts.append("(" + ring_str(close(orient_and_rotate(s))) + ")")
        return "MULTIPOLYGON(" + ",".join(parts) + ")" if parts else "MULTIPOLYGON EMPTY"

    def mpoly_touching():
        n = rng.randint(2, 4)
        parts = []
        for k in range(n):
            cs = [(k, 0), (k + 1, 0), (k + 1, 1), (k, 1)] if rng.random() < 0.7 else [(k, 0), (k + 1, 1), (k, 1)]
            parts.append("(" + ring_str(close(orient_and_rotate(cs))) + ")")
        return "MULTIPOLYGON(" + ",".join(parts) + ")"

    add("syn:mpoly_disjoint", mpoly_disjoint, 300)
    add("syn:mpoly_overlap", mpoly_overlap, 150)
    add("syn:mpoly_touching", mpoly_touching, 100)

    def any_simple(f):
        k = rng.randrange(4)
        if k == 0:
            return "POINT({} {})".format(*map(fmt, pt(f, rng.random(), rng.random())))
        if k == 1:
            return f"LINESTRING({coords_str(line_walk(f, rng.randint(2, 20)))})"
        if k == 2:
            return poly_valid_star(f)
        return multipoint(f)

    def gc():
        f = scale()
        k = rng.randrange(4)
        members = [any_simple(f) for _ in range(rng.randint(1, 5))]
        if k == 1:  # overlapping polygons (valid GC in PostGIS)
            members = [poly_valid_star(f), poly_valid_star(f)] + members[:1]
        elif k == 2:  # nested
            members = [f"GEOMETRYCOLLECTION({','.join(members)})", any_simple(f)]
        elif k == 3:  # empty members
            members = members + [rng.choice(["POINT EMPTY", "LINESTRING EMPTY", "POLYGON EMPTY", "GEOMETRYCOLLECTION EMPTY"])]
            rng.shuffle(members)
        return f"GEOMETRYCOLLECTION({','.join(members)})"

    add("syn:gc", gc, 400)

    empties = [
        "POINT EMPTY", "LINESTRING EMPTY", "POLYGON EMPTY", "MULTIPOINT EMPTY",
        "MULTILINESTRING EMPTY", "MULTIPOLYGON EMPTY", "GEOMETRYCOLLECTION EMPTY",
        "GEOMETRYCOLLECTION(POINT EMPTY)", "GEOMETRYCOLLECTION(POINT EMPTY,POINT(1 1))",
        "GEOMETRYCOLLECTION(LINESTRING EMPTY,POLYGON EMPTY)", "MULTIPOINT(EMPTY,(1 1))",
        "GEOMETRYCOLLECTION(GEOMETRYCOLLECTION EMPTY)", "MULTIPOLYGON(EMPTY,((0 0,1 0,1 1,0 0)))",
        "MULTILINESTRING(EMPTY,(0 0,1 1))",
    ]
    for _ in range(3):
        for e in empties:
            out.append(("syn:empty", e))
    return out


# ---------------------------------------------------------------------------------------------
# Database
# ---------------------------------------------------------------------------------------------


def main():
    DATA.mkdir(parents=True, exist_ok=True)
    with psycopg.connect(DSN, autocommit=True) as conn:
        cur = conn.cursor()
        cur.execute("DROP SCHEMA IF EXISTS e2 CASCADE")
        cur.execute("CREATE SCHEMA e2")
        cur.execute("CREATE TABLE e2.geom (id serial PRIMARY KEY, src text NOT NULL, geom geometry)")
        cur.execute("CREATE TABLE e2.pair (id serial PRIMARY KEY, src text NOT NULL, a geometry, b geometry)")

        # Real-world: NYC boroughs (fixture), as stored, and their polygons.
        t = pq.read_table(REPO / "fixtures/geoparquet/nybb_wkb.parquet")
        for name, wkb in zip(t.column("BoroName").to_pylist(), t.column("geometry").to_pylist()):
            cur.execute("INSERT INTO e2.geom (src, geom) VALUES ('real:nybb', ST_GeomFromWKB(%s))", (wkb,))
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'real:nybb_part', (ST_Dump(geom)).geom FROM e2.geom WHERE src = 'real:nybb' ORDER BY id""")

        # Real-world: Natural Earth (GeoJSON from nvkelso/natural-earth-vector).
        for layer in NE_LAYERS:
            fc = json.loads((NE / f"{layer}.geojson").read_text())
            rows = [json.dumps(f["geometry"]) for f in fc["features"] if f.get("geometry")]
            cur.executemany(
                "INSERT INTO e2.geom (src, geom) VALUES (%s, ST_SetSRID(ST_GeomFromGeoJSON(%s), 0))",
                [(f"real:{layer}", g) for g in rows],
            )

        # Synthetic, generated in Python.
        syn = synthetic()
        cur.executemany("INSERT INTO e2.geom (src, geom) VALUES (%s, ST_GeomFromText(%s))", syn)

        # Synthetic, derived in SQL from the Python ones (deterministic given the inputs and seeds).
        cur.execute("SELECT setseed(0.20261005)")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:makevalid', ST_MakeValid(geom) FROM e2.geom
            WHERE src IN ('syn:poly_random_order', 'syn:poly_special', 'syn:mpoly_overlap') ORDER BY id""")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:buffer', ST_Buffer(geom, (ST_XMax(geom) - ST_XMin(geom) + 1e-3) * (0.05 + (id % 7) / 10.0), 1 + id % 9)
            FROM e2.geom WHERE src IN ('syn:point', 'syn:line_walk') AND id % 4 = 0 ORDER BY id""")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:generatepoints', ST_GeneratePoints(geom, 1 + id % 40, id) FROM e2.geom
            WHERE src = 'syn:poly_star' AND id % 5 = 0 ORDER BY id""")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:snaptogrid', ST_SnapToGrid(geom, (ST_XMax(geom) - ST_XMin(geom)) / (3 + id % 20)) FROM e2.geom
            WHERE src IN ('syn:poly_star', 'syn:poly_holes', 'syn:line_walk') AND id % 3 = 0 ORDER BY id""")
        cur.execute("DELETE FROM e2.geom WHERE src = 'sql:snaptogrid' AND ST_IsEmpty(geom) AND id % 2 = 0")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:rotated_rect', ST_Rotate(geom, (id % 360) * pi() / 180, ST_Centroid(geom)) FROM e2.geom
            WHERE src = 'syn:poly_rect' ORDER BY id""")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:segmentize', ST_Segmentize(geom, (ST_XMax(geom) - ST_XMin(geom) + 1e-9) / 7) FROM e2.geom
            WHERE src IN ('syn:poly_rect', 'syn:line_special') AND id % 2 = 0 ORDER BY id""")
        cur.execute("""INSERT INTO e2.geom (src, geom)
            SELECT 'sql:difference', ST_Difference(a.geom, b.geom) FROM e2.geom a JOIN e2.geom b ON b.id = a.id + 1
            WHERE a.src = 'syn:poly_star' AND b.src = 'syn:poly_star' AND a.id % 4 = 0 AND a.geom && b.geom AND ST_IsValid(a.geom) AND ST_IsValid(b.geom) ORDER BY a.id""")

        cur.execute("DELETE FROM e2.geom WHERE geom IS NULL")

        # Per-row simplify tolerances: a fraction of the bbox size, chosen by id.
        cur.execute("ALTER TABLE e2.geom ADD COLUMN tol float8, ADD COLUMN vwtol float8")
        cur.execute("""UPDATE e2.geom SET tol = CASE WHEN ST_IsEmpty(geom) THEN 1 ELSE
            (ARRAY[0, 1e-4, 1e-3, 1e-2, 0.05, 0.2, 1.0])[1 + (id * 7919) % 7]
            * greatest(ST_XMax(geom) - ST_XMin(geom), ST_YMax(geom) - ST_YMin(geom)) END""")
        cur.execute("""UPDATE e2.geom SET vwtol = CASE WHEN ST_IsEmpty(geom) THEN 1 ELSE
            0.5 * power((ARRAY[0, 1e-4, 1e-3, 1e-2, 0.05, 0.2, 1.0])[1 + (id * 104729) % 7]
            * greatest(ST_XMax(geom) - ST_XMin(geom), ST_YMax(geom) - ST_YMin(geom)), 2) END""")

        # ---------------- pairs ----------------
        cur.execute("SELECT id FROM e2.geom ORDER BY id")
        ids = [r[0] for r in cur.fetchall()]
        prng = random.Random(SEED + 1)
        rand_pairs = [(prng.choice(ids), prng.choice(ids)) for _ in range(3000)]
        cur.executemany(
            "INSERT INTO e2.pair (src, a, b) SELECT 'random', a.geom, b.geom FROM e2.geom a, e2.geom b WHERE a.id = %s AND b.id = %s",
            rand_pairs,
        )
        # Random pairs within the same synthetic frame type are mostly disjoint; add near pairs:
        # consecutive synthetic geometries of the same source, translated onto each other.
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'near_synthetic', a.geom,
                ST_Translate(b.geom, ST_X(ST_Centroid(ST_Envelope(a.geom))) - ST_X(ST_Centroid(ST_Envelope(b.geom))),
                                     ST_Y(ST_Centroid(ST_Envelope(a.geom))) - ST_Y(ST_Centroid(ST_Envelope(b.geom))))
            FROM e2.geom a JOIN e2.geom b ON b.id = a.id + 1
            WHERE a.src LIKE 'syn:%' AND b.src LIKE 'syn:%' AND a.src <> 'syn:empty' AND b.src <> 'syn:empty'
              AND NOT ST_IsEmpty(a.geom) AND NOT ST_IsEmpty(b.geom) AND a.id % 3 = 0
            ORDER BY a.id""")
        # Neighbouring countries and states (shared borders).
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'ne_countries_bbox', a.geom, b.geom FROM e2.geom a JOIN e2.geom b
              ON a.src = 'real:ne_50m_admin_0_countries' AND b.src = a.src AND a.id < b.id AND a.geom && b.geom
            ORDER BY a.id, b.id""")
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'ne_states_bbox', a.geom, b.geom FROM e2.geom a JOIN e2.geom b
              ON a.src = 'real:ne_50m_admin_1_states_provinces' AND b.src = a.src AND a.id < b.id AND a.geom && b.geom
            WHERE (a.id * 31 + b.id) % 3 = 0
            ORDER BY a.id, b.id""")
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'ne_places_countries', b.geom, a.geom FROM e2.geom a JOIN e2.geom b
              ON a.src = 'real:ne_50m_admin_0_countries' AND b.src = 'real:ne_110m_populated_places' AND a.geom && b.geom
            ORDER BY a.id, b.id""")
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'ne_rivers_countries', a.geom, b.geom FROM e2.geom a JOIN e2.geom b
              ON a.src = 'real:ne_110m_admin_0_countries' AND b.src = 'real:ne_50m_rivers_lake_centerlines' AND a.geom && b.geom
            WHERE (a.id + b.id) % 2 = 0
            ORDER BY a.id, b.id""")
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'nybb_bbox', a.geom, b.geom FROM e2.geom a JOIN e2.geom b
              ON a.src LIKE 'real:nybb%' AND b.src LIKE 'real:nybb%' AND a.id <> b.id AND a.geom && b.geom
            ORDER BY a.id, b.id""")

        # Geometry vs a derived geometry: boundary, a vertex, a point along the boundary, centroid,
        # envelope, itself, reversed, slightly translated.
        cur.execute("""CREATE TEMP TABLE sample AS SELECT id, geom FROM e2.geom
            WHERE ST_Dimension(geom) > 0 AND NOT ST_IsEmpty(geom) AND GeometryType(geom) <> 'GEOMETRYCOLLECTION' AND id % 6 = 0""")
        for name, expr in [
            ("self", "geom"),
            ("reverse", "ST_Reverse(geom)"),
            ("boundary", "ST_Boundary(geom)"),
            ("vertex", "ST_PointN(ST_ExteriorRing(ST_GeometryN(geom, 1)), 1 + id % greatest(1, ST_NPoints(ST_ExteriorRing(ST_GeometryN(geom, 1))) - 1))"),
            ("line_vertex", "ST_PointN(ST_GeometryN(geom, 1), 1 + id % ST_NPoints(ST_GeometryN(geom, 1)))"),
            ("along_boundary", "ST_LineInterpolatePoint(CASE WHEN ST_Dimension(geom) = 2 THEN ST_GeometryN(ST_Boundary(geom), 1) ELSE ST_GeometryN(geom, 1) END, (id % 97) / 97.0)"),
            ("centroid", "ST_Centroid(geom)"),
            ("envelope", "ST_Envelope(geom)"),
            ("translated", "ST_Translate(geom, (ST_XMax(geom) - ST_XMin(geom)) * 0.3, 0)"),
        ]:
            if name == "vertex":
                where = "ST_Dimension(geom) = 2"
            elif name in ("line_vertex",):
                where = "GeometryType(geom) IN ('LINESTRING', 'MULTILINESTRING')"
            elif name == "along_boundary":
                where = "ST_Dimension(geom) = 2 OR GeometryType(geom) IN ('LINESTRING', 'MULTILINESTRING')"
            else:
                where = "true"
            cur.execute(f"""INSERT INTO e2.pair (src, a, b)
                SELECT 'derived:{name}', geom, {expr} FROM sample WHERE {where} ORDER BY id""")
            cur.execute(f"""INSERT INTO e2.pair (src, a, b)
                SELECT 'derived:{name}', {expr}, geom FROM sample WHERE {where} AND id % 12 = 0 ORDER BY id""")
        cur.execute("DELETE FROM e2.pair WHERE a IS NULL OR b IS NULL")

        # Small integer-grid pairs: exact touches, shared edges, collinear overlaps.
        grng = random.Random(SEED + 2)

        def grid_geom():
            k = grng.randrange(5)
            P = lambda: (grng.randint(0, 6), grng.randint(0, 6))  # noqa: E731
            if k == 0:
                return "POINT(%d %d)" % P()
            if k == 1:
                cs = [P() for _ in range(grng.randint(2, 4))]
                return "LINESTRING(" + coords_str(cs) + ")"
            if k == 2 or k == 3:
                x0, y0 = grng.randint(0, 4), grng.randint(0, 4)
                x1, y1 = x0 + grng.randint(1, 3), y0 + grng.randint(1, 3)
                if k == 2:
                    cs = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                else:
                    cs = [(x0, y0), (x1, y0), (x0, y1)]
                if grng.random() < 0.5:
                    cs = cs[::-1]
                return "POLYGON((" + coords_str(cs + [cs[0]]) + "))"
            cs = [P() for _ in range(grng.randint(1, 4))]
            return "MULTIPOINT(" + ",".join("(%d %d)" % c for c in cs) + ")"

        cur.executemany(
            "INSERT INTO e2.pair (src, a, b) VALUES ('grid', ST_GeomFromText(%s), ST_GeomFromText(%s))",
            [(grid_geom(), grid_geom()) for _ in range(2500)],
        )
        # EMPTY against everything.
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'empty', e.geom, g.geom FROM e2.geom e JOIN e2.geom g ON e.src = 'syn:empty' AND e.id < 1e9
            WHERE (g.id * 13 + e.id) % 389 = 0 ORDER BY e.id, g.id""")
        cur.execute("""INSERT INTO e2.pair (src, a, b)
            SELECT 'empty', g.geom, e.geom FROM e2.geom e JOIN e2.geom g ON e.src = 'syn:empty'
            WHERE (g.id * 17 + e.id) % 797 = 0 ORDER BY e.id, g.id""")

        # Export.
        with open(DATA / "unary.tsv", "w") as f:
            cur.execute("SELECT id, src, encode(ST_AsBinary(geom), 'hex'), tol, vwtol FROM e2.geom ORDER BY id")
            for r in cur:
                f.write("\t".join([str(r[0]), r[1], r[2], repr(r[3]), repr(r[4])]) + "\n")
        with open(DATA / "pairs.tsv", "w") as f:
            cur.execute("SELECT id, src, encode(ST_AsBinary(a), 'hex'), encode(ST_AsBinary(b), 'hex') FROM e2.pair ORDER BY id")
            for r in cur:
                f.write("\t".join([str(r[0]), r[1], r[2], r[3]]) + "\n")
        cur.execute("SELECT src, count(*) FROM e2.geom GROUP BY src ORDER BY src")
        print("unary:", sum(n for _, n in cur.fetchall()))
        cur.execute("SELECT count(*) FROM e2.pair")
        print("pairs:", cur.fetchone()[0])


if __name__ == "__main__":
    sys.exit(main())
