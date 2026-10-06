"""Evaluate the E2 functions with PostGIS over the corpus in schema `e2`.

Usage:
    uv run --with 'psycopg[binary]' python pg_eval.py

Writes <data>/pg/<function>.tsv with lines `id <TAB> value`, where value is hex WKB for
geometries, PostgreSQL's float8/boolean/text output otherwise, `NULL`, or `ERROR: <message>`.
"""

import os
import time
from pathlib import Path

import psycopg

REPO = Path(__file__).resolve().parents[2]
DATA = Path(os.environ.get("E2_DATA", REPO / "target/experiments/e2"))
DSN = os.environ.get("E2_DSN", "postgresql://postgres:postgres@localhost:54330/postgres")


def g(expr):
    return f"encode(ST_AsBinary({expr}), 'hex')"


UNARY = {
    "st_isvalid": "ST_IsValid(geom)::text",
    "st_pointonsurface": g("ST_PointOnSurface(geom)"),
    "st_convexhull": g("ST_ConvexHull(geom)"),
    "st_orientedenvelope": g("ST_OrientedEnvelope(geom)"),
    "st_simplify": g("ST_Simplify(geom, tol)"),
    "st_simplifyvw": g("ST_SimplifyVW(geom, vwtol)"),
    "st_centroid": g("ST_Centroid(geom)"),
    "st_area": "ST_Area(geom)::text",
    "st_length": "ST_Length(geom)::text",
}
BINARY = {
    "st_distance": "ST_Distance(a, b)::text",
    "st_contains": "ST_Contains(a, b)::text",
    "st_intersects": "ST_Intersects(a, b)::text",
    "st_within": "ST_Within(a, b)::text",
    "st_touches": "ST_Touches(a, b)::text",
    "st_relate": "ST_Relate(a, b)",
}


def connect():
    for _ in range(60):
        try:
            conn = psycopg.connect(DSN, autocommit=True)
            cur = conn.cursor()
            cur.execute("SET client_min_messages = warning")
            cur.execute("SET extra_float_digits = 1")  # shortest round-trip float8 output
            return cur
        except psycopg.OperationalError:
            time.sleep(1)
    raise RuntimeError("cannot connect")


def run(state, table, expr, ids):
    """Values for `ids`, falling back to smaller chunks (down to one row) on errors.

    A backend crash (segfault) loses the connection; reconnect and bisect to the crashing row.
    """
    out = {}

    def go(chunk):
        try:
            state["cur"].execute(f"SELECT id, {expr} FROM e2.{table} WHERE id = ANY(%s) ORDER BY id", (chunk,))
            for i, v in state["cur"].fetchall():
                out[i] = "NULL" if v is None else v
        except psycopg.Error as e:
            crashed = isinstance(e, psycopg.OperationalError)
            if crashed:
                state["cur"] = connect()
            if len(chunk) == 1:
                msg = "server crashed (connection lost)" if crashed else (e.diag.message_primary or str(e))
                msg = msg.replace("\t", " ").replace("\n", " ")
                out[chunk[0]] = f"ERROR: {msg}"
            else:
                mid = len(chunk) // 2
                go(chunk[:mid])
                go(chunk[mid:])

    for k in range(0, len(ids), 2000):
        go(ids[k : k + 2000])
    return out


def main():
    (DATA / "pg").mkdir(parents=True, exist_ok=True)
    state = {"cur": connect()}
    if True:
        cur = state["cur"]
        for table, funcs in (("geom", UNARY), ("pair", BINARY)):
            cur.execute(f"SELECT id FROM e2.{table} ORDER BY id")
            ids = [r[0] for r in cur.fetchall()]
            for name, expr in funcs.items():
                vals = run(state, table, expr, ids)
                cur = state["cur"]
                with open(DATA / "pg" / f"{name}.tsv", "w") as f:
                    for i in ids:
                        f.write(f"{i}\t{vals[i]}\n")
                errs = sum(v.startswith("ERROR") for v in vals.values())
                print(f"{name}: {len(vals)} rows, {errs} errors")
        # Validity of every input (for classification), and of each pair's members.
        cur.execute("SELECT id, ST_IsValid(geom) FROM e2.geom ORDER BY id")
        with open(DATA / "pg" / "_valid_unary.tsv", "w") as f:
            for i, v in cur.fetchall():
                f.write(f"{i}\t{str(v).lower()}\n")
        cur.execute("SELECT id, ST_IsValid(a), ST_IsValid(b) FROM e2.pair ORDER BY id")
        with open(DATA / "pg" / "_valid_pair.tsv", "w") as f:
            for i, a, b in cur.fetchall():
                f.write(f"{i}\t{str(a).lower()}\t{str(b).lower()}\n")


if __name__ == "__main__":
    main()
