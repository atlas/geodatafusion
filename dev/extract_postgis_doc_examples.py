#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["psycopg[binary]>=3.2"]
# ///
"""Generate sqllogictest files from the examples in the PostGIS reference documentation.

For every function documented in PostGIS' `doc/reference_*.xml`, this pulls the SQL out of the
`<programlisting>` blocks in its "Examples" section, keeps the statements that actually run on
PostGIS (examples referencing non-existent tables etc. are dropped), and writes them to
`rust/geodatafusion/tests/sqllogictests/slt/postgis_docs/<function>.slt` *without* expected
results. Then record the expected results from PostGIS with:

    cargo slt --complete postgis_docs

Usage:
    dev/postgis.sh start
    uv run dev/extract_postgis_doc_examples.py [--postgis-version 3.6.4] [--docs-dir DIR]

The PostGIS documentation is licensed under CC BY-SA 3.0; the generated files say so.
"""

from __future__ import annotations

import argparse
import io
import os
import re
import shutil
import sys
import tarfile
import urllib.request
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

import psycopg

REPO = Path(__file__).resolve().parent.parent
OUT_DIR = REPO / "rust/geodatafusion/tests/sqllogictests/slt/postgis_docs"
DEFAULT_URL = "postgresql://postgres:postgres@localhost:54329/postgres"
DB = "{http://docbook.org/ns/docbook}"
XML_ID = "{http://www.w3.org/XML/1998/namespace}id"

# Reference chapters that are out of scope (raster, SFCGAL, server admin) or whose examples
# depend on server state rather than geometry behaviour.
EXCLUDED_FILES = {
    "reference_raster.xml",
    "reference_sfcgal.xml",
    "reference_version.xml",
    "reference_guc.xml",
    "reference_troubleshooting.xml",
    "reference_management.xml",
    "reference_exception.xml",
}

# Statement kinds we keep. Anything else (UPDATE, ALTER, CREATE INDEX, SET, DO, ...) causes the
# whole example block to be dropped, because later statements usually depend on it.
QUERY_RE = re.compile(r"^\s*(select|with|values)\b", re.IGNORECASE)
ALLOWED_STATEMENT_RE = re.compile(
    r"^\s*(create\s+(temp(orary)?\s+)?table|insert\s+into|drop\s+table)\b",
    re.IGNORECASE,
)
STATEMENT_START_RE = re.compile(
    r"^\s*(select|with|values|create|insert|update|delete|drop|alter|set|do|truncate|"
    r"explain|analyze|vacuum|begin|commit|rollback|copy|table|grant|comment|prepare)\b",
    re.IGNORECASE,
)
# psql command tags such as "SELECT 3" or "INSERT 0 1" that look like the start of a statement.
COMMAND_TAG_RE = re.compile(
    r"^\s*(select|insert|update|delete)(\s+\d+)+\s*$", re.IGNORECASE
)
OUTPUT_RULE_RE = re.compile(r"^[\s\-+─┌┐└┘├┤┬┴┼│|]+$")
NONDETERMINISTIC_RE = re.compile(
    r"\b(now|random|clock_timestamp|statement_timestamp|gen_random_uuid|setseed)\s*\(",
    re.IGNORECASE,
)

MAX_ROWS = 100
MAX_RESULT_CHARS = 20_000


@dataclass
class Example:
    statements: list[str]


@dataclass
class Function:
    xml_id: str
    names: list[str]
    source_file: str
    examples: list[Example] = field(default_factory=list)

    @property
    def slug(self) -> str:
        return re.sub(r"[^a-z0-9_]+", "_", self.xml_id.lower()).strip("_")


def fetch_docs(version: str, cache: Path) -> Path:
    doc_dir = cache / f"postgis-{version}" / "doc"
    if doc_dir.exists():
        return doc_dir
    url = f"https://github.com/postgis/postgis/archive/refs/tags/{version}.tar.gz"
    print(f"Downloading {url}", file=sys.stderr)
    data = urllib.request.urlopen(url).read()
    cache.mkdir(parents=True, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        members = [
            m for m in tar.getmembers() if "/doc/" in m.name and m.name.endswith(".xml")
        ]
        tar.extractall(cache, members=members, filter="data")
    return doc_dir


def parse_xml(path: Path) -> ET.Element:
    text = path.read_text()
    # Drop DocBook entities defined in the (unavailable) DTD, e.g. &Z_support;.
    text = re.sub(
        r"&(?!(amp|lt|gt|quot|apos|#\d+|#x[0-9a-fA-F]+);)[A-Za-z_][\w.-]*;", "", text
    )
    return ET.fromstring(text)


def strip_comments(sql: str) -> str:
    lines = [l for l in sql.splitlines() if not l.strip().startswith("--")]
    return "\n".join(lines).strip()


def split_sql(text: str) -> list[str]:
    """Split a programlisting into chunks ending in `;`, respecting quotes and comments."""
    chunks, buf = [], []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "'":
            j = i + 1
            while j < n:
                if text[j] == "'" and j + 1 < n and text[j + 1] == "'":
                    j += 2
                    continue
                if text[j] == "'":
                    break
                j += 1
            buf.append(text[i : j + 1])
            i = j + 1
            continue
        if text.startswith("--", i):
            j = text.find("\n", i)
            j = n if j == -1 else j
            buf.append(text[i:j])
            i = j
            continue
        if c == ";":
            chunks.append("".join(buf))
            buf = []
        else:
            buf.append(c)
        i += 1
    chunks.append("".join(buf))  # trailing text without `;`
    return chunks


def statement_from_chunk(chunk: str, terminated: bool) -> str | None:
    """Find where the SQL starts in a chunk (preceded by the previous statement's output)."""
    lines = chunk.splitlines()
    start = None
    for idx, line in enumerate(lines):
        if line.strip().startswith("--"):
            continue
        if STATEMENT_START_RE.match(line) and not COMMAND_TAG_RE.match(line):
            start = idx
            break
    if start is None:
        return None
    body = lines[start:]
    if not terminated:
        # No `;`: assume the statement ends at the first blank line or output header.
        end = len(body)
        for idx, line in enumerate(body):
            if not line.strip() or OUTPUT_RULE_RE.match(line):
                end = idx
                break
            # A column header line is followed by a rule like `-----+----`.
            nxt = body[idx + 1] if idx + 1 < len(body) else ""
            if nxt.strip() and OUTPUT_RULE_RE.match(nxt):
                end = idx
                break
        body = body[:end]
    sql = strip_comments("\n".join(body))
    return sql or None


def extract_statements(listing: str) -> list[str]:
    chunks = split_sql(listing)
    out = []
    for idx, chunk in enumerate(chunks):
        terminated = idx < len(chunks) - 1
        sql = statement_from_chunk(chunk, terminated)
        if sql:
            out.append(sql)
    return out


def section_title(section: ET.Element) -> str:
    title = section.find(f"{DB}title")
    return "".join(title.itertext()).strip() if title is not None else ""


def extract_functions(doc_dir: Path) -> list[Function]:
    functions = []
    for path in sorted(doc_dir.glob("reference_*.xml")):
        if path.name in EXCLUDED_FILES:
            continue
        root = parse_xml(path)
        for entry in root.iter(f"{DB}refentry"):
            xml_id = entry.get(XML_ID)
            names = ["".join(n.itertext()).strip() for n in entry.iter(f"{DB}refname")]
            if not xml_id or not names:
                continue
            fn = Function(xml_id=xml_id, names=names, source_file=path.name)
            for section in entry.iter(f"{DB}refsection"):
                if "example" not in section_title(section).lower():
                    continue
                for listing in section.iter(f"{DB}programlisting"):
                    statements = extract_statements("".join(listing.itertext()))
                    if statements:
                        fn.examples.append(Example(statements))
            functions.append(fn)
    return functions


@dataclass
class ValidRecord:
    sql: str
    is_query: bool
    rowsort: bool


def validate(conn: psycopg.Connection, fn: Function) -> list[list[ValidRecord]]:
    """Run the examples against PostGIS in one rolled-back transaction, keeping blocks that work."""
    kept = []
    with conn.transaction(force_rollback=True):
        for example in fn.examples:
            records = []
            ok = True
            try:
                with conn.transaction():  # savepoint
                    for sql in example.statements:
                        if NONDETERMINISTIC_RE.search(sql):
                            raise ValueError("nondeterministic")
                        if QUERY_RE.match(sql):
                            rows = run_query(conn, sql)
                            if rows is None or rows != run_query(conn, sql):
                                raise ValueError("unstable or oversized result")
                            ordered = (
                                re.search(r"\border\s+by\b", sql, re.IGNORECASE)
                                is not None
                            )
                            records.append(
                                ValidRecord(sql, True, len(rows) > 1 and not ordered)
                            )
                        elif ALLOWED_STATEMENT_RE.match(sql):
                            conn.execute(sql)
                            records.append(ValidRecord(sql, False, False))
                        else:
                            raise ValueError("unsupported statement kind")
            except (psycopg.Error, ValueError):
                ok = False
            # Keep setup-only blocks too: their effects persist in this transaction, so later
            # blocks may depend on them.
            if ok and records:
                kept.append(records)
    return kept


def run_query(conn: psycopg.Connection, sql: str):
    cur = conn.execute(sql)
    if cur.description is None:
        return None
    rows = cur.fetchmany(MAX_ROWS + 1)
    if len(rows) > MAX_ROWS or len(repr(rows)) > MAX_RESULT_CHARS:
        return None
    return rows


def render_slt(fn: Function, blocks: list[list[ValidRecord]], version: str) -> str:
    names = ", ".join(fn.names)
    lines = [
        f"# PostGIS documentation examples for {names}",
        f"# Source: PostGIS {version} doc/{fn.source_file} (https://postgis.net/docs/{fn.xml_id}.html)",
        "# The SQL in this file is derived from the PostGIS documentation, licensed CC BY-SA 3.0.",
        "# Generated by dev/extract_postgis_doc_examples.py; expected results recorded from PostGIS",
        "# with `--complete`. Do not edit by hand; regenerate instead.",
        "",
    ]
    for i, block in enumerate(blocks, 1):
        lines.append(f"# Example {i}")
        for rec in block:
            # sqllogictest uses blank lines as record separators.
            sql = "\n".join(l for l in rec.sql.splitlines() if l.strip())
            if rec.is_query:
                lines.append("query T rowsort" if rec.rowsort else "query T")
            else:
                lines.append("statement ok")
            lines.append(sql)
            lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--postgis-version", default="3.6.4")
    parser.add_argument(
        "--docs-dir", type=Path, help="Use an existing PostGIS doc/ directory"
    )
    parser.add_argument(
        "--postgis-url", default=os.environ.get("POSTGIS_URL", DEFAULT_URL)
    )
    parser.add_argument(
        "--only", nargs="*", help="Only (re)generate these refentry ids, e.g. ST_Area"
    )
    args = parser.parse_args()

    doc_dir = args.docs_dir or fetch_docs(
        args.postgis_version, REPO / "target/postgis-docs"
    )
    functions = extract_functions(doc_dir)
    if args.only:
        wanted = {o.lower() for o in args.only}
        functions = [
            f for f in functions if f.xml_id.lower() in wanted or f.slug in wanted
        ]

    if not args.only and OUT_DIR.exists():
        shutil.rmtree(OUT_DIR)
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    without_examples = []
    total_records = 0
    with psycopg.connect(args.postgis_url, autocommit=True) as conn:
        conn.execute("SET client_min_messages = warning")
        conn.execute("SET statement_timeout = '10s'")
        for fn in functions:
            blocks = validate(conn, fn)
            if not blocks:
                without_examples.append(fn)
                continue
            total_records += sum(len(b) for b in blocks)
            (OUT_DIR / f"{fn.slug}.slt").write_text(
                render_slt(fn, blocks, args.postgis_version)
            )

    if not args.only:
        write_readme(functions, without_examples, args.postgis_version)
    print(
        f"Wrote {len(functions) - len(without_examples)} files ({total_records} records); "
        f"{len(without_examples)} functions had no usable examples.",
        file=sys.stderr,
    )
    print(
        "Now record expected results:\n  cargo slt --complete postgis_docs",
        file=sys.stderr,
    )
    return 0


def write_readme(
    functions: list[Function], without: list[Function], version: str
) -> None:
    lines = [
        "# PostGIS documentation examples",
        "",
        f"Generated from the PostGIS {version} reference documentation by",
        "`dev/extract_postgis_doc_examples.py`. Expected results are recorded from PostGIS",
        "itself. **Do not edit these files by hand**; add hand-written tests under",
        "`../geodatafusion/` instead.",
        "",
        "The SQL is derived from the [PostGIS documentation](https://postgis.net/docs/),",
        "which is licensed under [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/).",
        "",
        f"## Functions without usable examples ({len(without)})",
        "",
        "These have no examples that run standalone on PostGIS (they need tables, are",
        "nondeterministic, or have no examples at all). Write tests for them by hand.",
        "",
    ]
    for fn in sorted(without, key=lambda f: f.xml_id.lower()):
        lines.append(f"- {', '.join(fn.names)} (`{fn.source_file}`)")
    (OUT_DIR / "README.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
