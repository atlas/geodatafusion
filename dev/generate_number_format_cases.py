#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["psycopg[binary]>=3.2"]
# ///
"""Generate the test cases for geodatafusion's PostGIS number formatter.

Writes `value|maxdecimaldigits|PostGIS output` lines to
`rust/geodatafusion/src/udf/native/io/util/testdata/number_format.txt`, with the output taken
from PostGIS's ST_AsText. The cases are seeded random doubles over many magnitudes, decimal
ties, values around the fixed/exponential boundaries (1e-8 and 1e15), and special values.

Usage:
    dev/postgis.sh start
    uv run dev/generate_number_format_cases.py
"""

from __future__ import annotations

import os
import random
import sys
from pathlib import Path

import psycopg

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "rust/geodatafusion/src/udf/native/io/util/testdata/number_format.txt"
DEFAULT_URL = "postgresql://postgres:postgres@localhost:54329/postgres"
PRECISIONS = [-1, 0, 1, 2, 3, 5, 8, 10, 12, 15, 17, 20]


def cases(rng: random.Random) -> list[tuple[float, int]]:
    values: list[float] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        1.5,
        2.5,
        0.125,
        0.375,
        2.675,
        0.45,
    ]
    # Random doubles over many magnitudes, with full and short mantissas.
    for _ in range(1500):
        exponent = rng.uniform(-13, 18)
        value = rng.choice([1, -1]) * 10**exponent
        values.append(value)
        values.append(float(f"{value:.{rng.randint(1, 6)}g}"))
    # Decimal ties: d.ddd5 at several scales.
    for _ in range(300):
        digits = rng.randint(1, 6)
        scale = rng.randint(-9, 12)
        mantissa = rng.randint(0, 10**digits - 1) * 10 + 5
        values.append(rng.choice([1, -1]) * mantissa * 10.0 ** (scale - digits - 1))
    # Around the fixed/exponential boundaries.
    for boundary in (1e-8, 1e15):
        for factor in (0.99, 0.999999, 1.0, 1.000001, 1.01, 9.99, 0.0999):
            values.append(boundary * factor)
            values.append(-boundary * factor)
    return [(value, rng.choice(PRECISIONS)) for value in values] + [
        (value, precision) for value in values[:11] for precision in PRECISIONS
    ]


def main() -> int:
    rng = random.Random(20261006)
    rows = cases(rng)
    url = os.environ.get("POSTGIS_URL", DEFAULT_URL)
    with psycopg.connect(url) as conn:
        version = conn.execute("SELECT postgis_lib_version()").fetchone()[0]
        results = conn.execute(
            """
            SELECT ST_AsText(ST_MakePoint(v, 0), p)
            FROM unnest(%s::float8[], %s::int[]) WITH ORDINALITY AS t(v, p, i)
            ORDER BY i
            """,
            ([v for v, _ in rows], [p for _, p in rows]),
        ).fetchall()

    lines = [
        f"# PostGIS {version} ST_AsText coordinates: value|maxdecimaldigits|output"
    ]
    for (value, precision), (text,) in zip(rows, results, strict=True):
        # 'POINT(<x> 0)' -> '<x>'
        x = text.removeprefix("POINT(").removesuffix(")").rsplit(" ", 1)[0]
        lines.append(f"{value!r}|{precision}|{x}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"Wrote {len(rows)} cases to {OUT.relative_to(REPO)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
