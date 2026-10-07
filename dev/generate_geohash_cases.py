#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["psycopg[binary]>=3.2"]
# ///
"""Generate the test cases for geodatafusion's PostGIS GeoHash codec.

Writes `rust/geodatafusion/src/udf/native/io/util/testdata/geohash.txt`, with the results taken
from PostGIS:

    encode|<xmin ymin>|<xmax ymax>|<maxchars>|<ST_GeoHash of the box's diagonal>
    decode|<hash>|<precision>|<ST_AsText(ST_PointFromGeoHash)>|<ST_AsText(ST_GeomFromGeoHash)>

A GeoHash result is `(empty)` for the empty string and `ERROR` for an error; a precision is
`NULL` when not given. The cases are seeded random points and boxes over many scales, points on
cell boundaries, out-of-range coordinates, and random hashes up to 24 characters.

Usage:
    dev/postgis.sh start
    uv run dev/generate_geohash_cases.py
"""

from __future__ import annotations

import os
import random
import sys
from pathlib import Path

import psycopg

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "rust/geodatafusion/src/udf/native/io/util/testdata/geohash.txt"
DEFAULT_URL = "postgresql://postgres:postgres@localhost:54329/postgres"
BASE32 = "0123456789bcdefghjkmnpqrstuvwxyz"
MAXCHARS = [-1, 0, 1, 2, 5, 8, 12, 20, 25, 30]


def encode_cases(rng: random.Random) -> list[tuple[float, float, float, float, int]]:
    cases = []
    # Points, including cell boundaries and the corners of the world.
    for x, y in [(0, 0), (180, 90), (-180, -90), (90, 45), (-90, -45), (22.5, 11.25), (-126, 48)]:
        for maxchars in MAXCHARS:
            cases.append((x, y, x, y, maxchars))
    for _ in range(800):
        x, y = rng.uniform(-180, 180), rng.uniform(-90, 90)
        cases.append((x, y, x, y, rng.choice(MAXCHARS)))
    # Boxes from one degree wide to almost a point, some on a cell boundary.
    for _ in range(1200):
        x, y = rng.uniform(-179, 179), rng.uniform(-89, 89)
        if rng.random() < 0.1:
            x, y = rng.choice([0.0, 45.0, -90.0, 22.5]), rng.choice([0.0, 45.0, -22.5])
        w, h = 10 ** rng.uniform(-9, 0), 10 ** rng.uniform(-9, 0)
        cases.append((x, y, x + w, y + h, rng.choice(MAXCHARS)))
    # Out of range.
    for x1, y1, x2, y2 in [(200, 0, 200, 0), (-180.0000001, 0, -180.0000001, 0), (0, 90.5, 0, 90.5), (-200, 0, 0, 0)]:
        cases.append((x1, y1, x2, y2, 0))
    return cases


def decode_cases(rng: random.Random) -> list[tuple[str, int | None]]:
    cases: list[tuple[str, int | None]] = [("", None), ("9qqj7nmxncgyy4d0dbxqz0", None), ("9QQJ", None)]
    for _ in range(800):
        length = rng.randint(1, 24)
        hash_ = "".join(rng.choice(BASE32) for _ in range(length))
        precision = rng.choice([None, None, -1, 0, 1, 3, length, length + 5])
        cases.append((hash_, precision))
    return cases


def main() -> int:
    rng = random.Random(20261007)
    url = os.environ.get("POSTGIS_URL", DEFAULT_URL)
    lines = []
    with psycopg.connect(url, autocommit=True) as conn:
        version = conn.execute("SELECT postgis_lib_version()").fetchone()[0]
        lines.append(f"# PostGIS {version} GeoHash encoding and decoding; see dev/generate_geohash_cases.py")
        for x1, y1, x2, y2, maxchars in encode_cases(rng):
            try:
                (hash_,) = conn.execute(
                    "SELECT ST_GeoHash(ST_MakeLine(ST_Point(%s, %s), ST_Point(%s, %s)), %s)",
                    (x1, y1, x2, y2, maxchars),
                ).fetchone()
                result = hash_ if hash_ else "(empty)"
            except psycopg.Error:
                result = "ERROR"
            lines.append(f"encode|{x1!r} {y1!r}|{x2!r} {y2!r}|{maxchars}|{result}")
        for hash_, precision in decode_cases(rng):
            point, polygon = conn.execute(
                "SELECT ST_AsText(ST_PointFromGeoHash(%s, %s)), ST_AsText(ST_GeomFromGeoHash(%s, %s))",
                (hash_, precision, hash_, precision),
            ).fetchone()
            shown = "NULL" if precision is None else precision
            lines.append(f"decode|{hash_}|{shown}|{point}|{polygon}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"Wrote {len(lines) - 1} cases to {OUT.relative_to(REPO)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
