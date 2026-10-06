"""H8: aggregate vs scalar forms of ST_Collect, ST_MakeLine and ST_Union in public code.

Sample: GitHub code search for '"<name>" language:<lang>' — SQL pages 1-2, PLpgSQL page 1 and
Python page 1 (up to 400 files per function, best-match order). The full file at the indexed
commit is downloaded from raw.githubusercontent.com and every call of the function is parsed
(balanced parentheses, quotes respected) and classified:

- aggregate: one argument that isn't an array (ARRAY[...], ARRAY(...), array_agg(...)), or any
  call followed by OVER (window use), or ST_Union with two arguments where the second is a
  numeric literal (the gridSize aggregate);
- scalar-array: one argument that is an array constructor (ARRAY[...], ARRAY(...), array_agg(...),
  a BigQuery-style [...] literal, or a ::geometry[] cast);
- ambiguous: one bare-variable argument in a PL/pgSQL assignment (x := ST_MakeLine(pts)), which
  is usually a geometry[] variable; left out of the share;
- scalar: two or more geometry arguments (ST_Union(g1, g2, gridSize) counts as scalar);
- excluded: definitions (CREATE FUNCTION/AGGREGATE ... name), DROP statements, SQL line
  comments, vendored PostGIS sources, and duplicate files (same content hash).

Usage: uv run --with requests python h8_aggregate_vs_scalar.py [--fetch-only]
"""

from __future__ import annotations

import collections
import hashlib
import re
import sys

import common
from analyze_h15 import is_vendored

FUNCS = ["ST_Collect", "ST_MakeLine", "ST_Union"]
PAGES = [("SQL", 1), ("SQL", 2), ("PLpgSQL", 1), ("Python", 1)]
NUM = re.compile(r"^[+-]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?(::\w+)?$", re.I)


def strip_sql_comments(text: str) -> str:
    return re.sub(r"--[^\n]*", "", text)


def parse_args(text: str, i: int) -> tuple[list[str], int] | None:
    """text[i] == '('. Return top-level arguments and the index after the closing paren."""
    depth = 0
    args, cur = [], []
    quote = None
    j = i
    while j < len(text):
        c = text[j]
        if quote:
            cur.append(c)
            if c == quote:
                quote = None
        elif c in ("'", '"'):
            quote = c
            cur.append(c)
        elif c in "([":
            depth += 1
            if depth > 1:
                cur.append(c)
        elif c in ")]":
            depth -= 1
            if depth == 0:
                args.append("".join(cur).strip())
                return args, j + 1
            cur.append(c)
        elif c == "," and depth == 1:
            args.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        j += 1
        if j - i > 20000:
            return None
    return None


def classify(name: str, text: str, pos: int, m_end: int) -> str | None:
    before = text[max(0, pos - 80):pos]
    if re.search(r"(function|aggregate|procedure)\s+(if\s+exists\s+)?([\w\"]+\.)?\"?$", before, re.I):
        return None
    parsed = parse_args(text, m_end - 1)
    if parsed is None:
        return None
    args, after = parsed
    args = [a for a in args if a != ""]
    tail = text[after:after + 40]
    if re.match(r"\s*(filter\s*\([^)]*\)\s*)?over\b", tail, re.I):
        return "aggregate"
    if not args:
        return None
    first = args[0]
    if any(re.match(r"\s*(\w+\s*\(\s*)?(order\s+by)", a, re.I) for a in args[1:]) or re.search(r"\border\s+by\b", first, re.I):
        return "aggregate"
    if len(args) == 1:
        if (re.match(r"(array\s*[\[(]|array_agg\s*\(|\[)", first, re.I)
                or re.search(r"::\s*geometry\s*\[\]$", first, re.I)):
            return "scalar-array"
        # A bare variable in a PL/pgSQL assignment (x := ST_MakeLine(pts)) is usually a
        # geometry[] variable, so the form can't be told from the text.
        stmt = text[max(0, pos - 200):pos].rsplit(";", 1)[-1]
        if re.fullmatch(r"[\w.]+", first) and ":=" in stmt and not re.search(r"\bselect\b", stmt, re.I):
            return "ambiguous"
        return "aggregate"
    if name == "ST_Union" and len(args) == 2 and NUM.match(args[1]):
        return "aggregate"
    if name == "ST_Union" and len(args) <= 3:
        return "scalar"
    if len(args) == 2:
        return "scalar"
    return "other"


def main() -> None:
    fetch_only = "--fetch-only" in sys.argv
    results = {}
    for name in FUNCS:
        calls = collections.Counter()
        files = collections.Counter()
        seen = set()
        per_lang = collections.defaultdict(collections.Counter)
        n_files = 0
        examples = collections.defaultdict(list)
        for lang, page in PAGES:
            d = common.gh_code_search(f'"{name}" language:{lang}', page=page)
            for it in d.get("items", []):
                if is_vendored(it):
                    files["vendored"] += 1
                    continue
                sha = re.search(r"/blob/([0-9a-f]{40})/", it["html_url"])
                ref = sha.group(1) if sha else "HEAD"
                text = common.raw_file(it["repository"]["full_name"], ref, it["path"])
                if fetch_only or not text:
                    continue
                h = hashlib.sha1(text.encode()).hexdigest()
                if h in seen:
                    files["duplicate"] += 1
                    continue
                seen.add(h)
                n_files += 1
                body = strip_sql_comments(text) if lang != "Python" else text
                kinds = collections.Counter()
                for m in re.finditer(rf"(?<![\w$]){name}\s*\(", body, re.I):
                    k = classify(name, body, m.start(), m.end())
                    if k:
                        kinds[k] += 1
                        if len(examples[k]) < 4:
                            examples[k].append(f"{it['repository']['full_name']}/{it['path']}: "
                                               + " ".join(body[m.start():m.start() + 90].split()))
                calls.update(kinds)
                per_lang[lang].update(kinds)
                if not kinds:
                    files["no-call"] += 1
                elif kinds["aggregate"] and (kinds["scalar"] or kinds["scalar-array"]):
                    files["both"] += 1
                elif kinds["aggregate"]:
                    files["aggregate-only"] += 1
                else:
                    files["scalar-only"] += 1
        results[name] = (n_files, calls, files, per_lang, examples)
    if fetch_only:
        return
    print("| Function | Files parsed | Aggregate calls | Scalar (2+ args) | Scalar (array) | Ambiguous | Other | Aggregate share | Files agg-only / scalar-only / both / no call | Excluded (vendored / duplicate) |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for name, (n_files, calls, files, per_lang, examples) in results.items():
        tot = calls["aggregate"] + calls["scalar"] + calls["scalar-array"]
        share = calls["aggregate"] / tot if tot else float("nan")
        print(f"| {name} | {n_files} | {calls['aggregate']} | {calls['scalar']} | {calls['scalar-array']} | "
              f"{calls['ambiguous']} | {calls['other']} | {share:.0%} | {files['aggregate-only']} / {files['scalar-only']} / "
              f"{files['both']} / {files['no-call']} | {files['vendored']} / {files['duplicate']} |")
    print()
    for name, (n_files, calls, files, per_lang, examples) in results.items():
        print(name, {lang: dict(c) for lang, c in per_lang.items()})
        for k, ex in examples.items():
            for e in ex:
                print(f"   [{k}] {e}")


if __name__ == "__main__":
    main()
