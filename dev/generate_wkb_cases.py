#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["psycopg[binary]>=3.2"]
# ///
"""Generate the test cases for geodatafusion's PostGIS (E)WKB writer.

Writes `rust/geodatafusion/src/udf/native/io/util/testdata/wkb.txt` with
`ewkt|endianness|ST_AsHEXEWKB|hex of ST_AsBinary` lines taken from PostGIS, for every geometry
type, dimension and EMPTY form, with and without an SRID, in both byte orders.

Usage:
    dev/postgis.sh start
    uv run dev/generate_wkb_cases.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

import psycopg

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "rust/geodatafusion/src/udf/native/io/util/testdata/wkb.txt"
DEFAULT_URL = "postgresql://postgres:postgres@localhost:54329/postgres"

GEOMETRIES = [
    "POINT(1 2)",
    "POINT EMPTY",
    "LINESTRING(0 0,1 1,2 0.5)",
    "LINESTRING EMPTY",
    "POLYGON((0 0,10 0,10 10,0 0),(1 1,2 1,2 2,1 1))",
    "POLYGON EMPTY",
    "MULTIPOINT((1 2),(3 4))",
    "MULTIPOINT((1 2),EMPTY)",
    "MULTIPOINT EMPTY",
    "MULTILINESTRING((0 0,1 1),(2 2,3 3,4 4))",
    "MULTILINESTRING EMPTY",
    "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))",
    "MULTIPOLYGON EMPTY",
    "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1),POLYGON EMPTY)",
    # Not nested collections: GeoArrow can't hold them.
    "GEOMETRYCOLLECTION(MULTIPOINT((3 4)),POINT EMPTY)",
    "GEOMETRYCOLLECTION EMPTY",
]
# PostGIS adds the dimensions, so the cases cover its own EMPTY and nested forms.
DIMENSIONS = ["g", "ST_Force3DZ(g, 3)", "ST_Force3DM(g, 4)", "ST_Force4D(g, 3, 4)"]


def main() -> int:
    url = os.environ.get("POSTGIS_URL", DEFAULT_URL)
    lines = []
    with psycopg.connect(url, autocommit=True) as conn:
        version = conn.execute("SELECT postgis_lib_version()").fetchone()[0]
        lines.append(f"# PostGIS {version} (E)WKB output: ewkt|endianness|ST_AsHEXEWKB|ST_AsBinary as hex")
        for wkt in GEOMETRIES:
            for dim in DIMENSIONS:
                for srid in [0, 4326]:
                    for endianness in ["NDR", "XDR"]:
                        text, hexewkb, hexwkb = conn.execute(
                            f"SELECT ST_AsText(d), ST_AsHEXEWKB(d, %s), encode(ST_AsBinary(d, %s), 'hex') "
                            f"FROM (SELECT {dim} AS d FROM ST_SetSRID(ST_GeomFromText(%s), %s) AS g) AS t",
                            (endianness, endianness, wkt, srid),
                        ).fetchone()
                        ewkt = f"SRID={srid};{text}" if srid else text
                        lines.append(f"{ewkt}|{endianness}|{hexewkb}|{hexwkb.upper()}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"Wrote {len(lines) - 1} cases to {OUT.relative_to(REPO)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
