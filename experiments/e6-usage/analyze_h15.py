"""H15: rank every searchable inventory function by public-code usage.

Reads cached GitHub code search responses (fetch_h15.py) and Stack Exchange counts (fetch_se.py).
For each function and language the raw `total_count` is corrected by the precision measured on
the first page of results (up to 100 files): a file counts as a true use if at least one text
match is the exact function name (case-insensitive), not preceded by an identifier character or
a dot, followed by optional whitespace and "(", and the file isn't a definition of the function
(CREATE FUNCTION / create or replace function x.name) or a vendored copy of PostGIS's own sources.

Writes usage.csv and prints summary tables.

Usage: uv run --with requests --with scipy python analyze_h15.py
"""

from __future__ import annotations

import csv
import re
import sys

from scipy.stats import spearmanr

import common
from fetch_se import SKIP

LANGS = ["SQL", "PLpgSQL", "Python"]
PRIMARY = ["SQL", "PLpgSQL"]
IDENT = re.compile(r"[A-Za-z0-9_$]")
DEF_BEFORE = re.compile(r"(function|aggregate|procedure)\s+(if\s+exists\s+)?([\w\"]+\.)?[\"]?$", re.I)
VENDORED_PATH = re.compile(
    r"(^|/)(regress|doc|extensions/postgis\w*|postgis/sql|topology|raster)/|postgis\w*--[\d.]+|"
    r"(^|/)(postgis|legacy\w*|uninstall_\w+|postgis_upgrade\w*|rtpostgis\w*|spatial_ref_sys|sfcgal\w*)\.sql(\.in)?$",
    re.I)


def is_vendored(item: dict) -> bool:
    repo = item["repository"]["full_name"].lower()
    path = item["path"]
    if "postgis" in repo and VENDORED_PATH.search(path):
        return True
    # Copies of PostGIS (or a port such as openGauss's) regression suites and sources inside other
    # repositories, and full schema dumps that carry PostGIS's function definitions.
    return bool(re.search(r"postgis\w*--[\d.]+.*\.sql$|(^|/)postgis(_comments)?\.sql$|(^|/)legacy(_minimal)?\.sql$|"
                          r"(^|/)regress/|postgis[-_][\d.]+(svn)?/|postgis_source/|contrib/postgis/|"
                          r"postgis_function\w*\.sql$|(^|/)(development_|test_)?structure\.sql$",
                          path, re.I))


def split_match(fragment: str, start: int, end: int) -> tuple[str, str, str]:
    """GitHub's text-match indices are UTF-8 byte offsets into the fragment."""
    b = fragment.encode()
    return (b[:start].decode(errors="ignore"), b[start:end].decode(errors="ignore"),
            b[end:].decode(errors="ignore"))


def match_is_use(name: str, fragment: str, start: int, end: int) -> bool:
    """Exact-name call at bytes [start, end) of the fragment. A schema prefix (public.st_x) or
    an ORM prefix (func.ST_X) is allowed; an identifier character right before isn't."""
    before, text, rest = split_match(fragment, start, end)
    if text.lower() != name.lower():
        return False
    if before and IDENT.match(before[-1]):
        return False
    if not re.match(r"\s*\(", rest):
        return False
    if DEF_BEFORE.search(before[-60:]):
        return False
    return True


def item_class(name: str, item: dict) -> str:
    """'use', 'definition', 'vendored' or 'noise' for one search result file."""
    if is_vendored(item):
        return "vendored"
    saw_def = False
    for tm in item.get("text_matches", []):
        frag = tm.get("fragment", "")
        for m in tm.get("matches", []):
            s, e = m["indices"]
            before, text, _ = split_match(frag, s, e)
            if text.lower() != name.lower():
                continue
            if match_is_use(name, frag, s, e):
                return "use"
            if DEF_BEFORE.search(before[-60:]):
                saw_def = True
    return "definition" if saw_def else "noise"


def lang_stats(name: str, lang: str) -> dict | None:
    q = f'"{name}" language:{lang}'
    path = common._cache_path("ghcode", __import__("json").dumps(
        ["https://api.github.com/search/code", {"q": q, "per_page": 100, "page": 1},
         {"Accept": "application/vnd.github.text-match+json"}], sort_keys=True))
    if not path.exists():
        return None
    d = common.gh_code_search(q)
    if "items" not in d:
        return {"total": 0, "n": 0, "use": 0, "precision": 0.0, "adjusted": 0.0, "classes": {},
                "repos": 0, "repo_adjusted": 0.0, "low": False}
    classes: dict[str, int] = {}
    repos: set[str] = set()
    for it in d["items"]:
        c = item_class(name, it)
        classes[c] = classes.get(c, 0) + 1
        if c == "use":
            repos.add(it["repository"]["full_name"])
    n = len(d["items"])
    use = classes.get("use", 0)
    prec = use / n if n else 0.0
    total = d["total_count"]
    # When every result was sampled the corrected count is exact for the index.
    adjusted = use if total <= n else total * prec
    # Sensitivity metric: scale by the share of distinct repositories among the true-use files in
    # the sample, so one repository with dozens of generated files doesn't count dozens of times.
    repo_adjusted = adjusted * len(repos) / use if use else 0.0
    # Low confidence: the precision estimate rests on few true uses relative to a large raw count.
    low = total > 2 * n and (use < 10 or prec < 0.3)
    return {"total": total, "n": n, "use": use, "precision": prec, "adjusted": adjusted, "classes": classes,
            "repos": len(repos), "repo_adjusted": repo_adjusted, "low": low}


def se_counts() -> dict[str, int]:
    out = {}
    for r in common.inventory():
        key_params = {"q": r["name"], "site": "gis", "filter": "total"}
        p = common._cache_path("se", __import__("json").dumps(
            ["https://api.stackexchange.com/2.3/search/excerpts", key_params, {}], sort_keys=True))
        if p.exists():
            out[r["name"]] = __import__("json").loads(p.read_text())["response"].get("total")
    return out


def percentile_ranks(values: list[float]) -> list[float]:
    """Percent of functions with a strictly lower score, plus half the ties (mid-rank)."""
    n = len(values)
    out = []
    for v in values:
        lower = sum(1 for w in values if w < v)
        equal = sum(1 for w in values if w == v) - 1
        out.append(100.0 * (lower + equal / 2) / (n - 1))
    return out


def main() -> None:
    rows = [r for r in common.inventory() if common.searchable(r)]
    se = se_counts()
    table = []
    for r in rows:
        rec = dict(r)
        for lang in LANGS:
            s = lang_stats(r["name"], lang)
            rec[lang] = s
        if any(rec[lang] is None for lang in PRIMARY):
            print("missing data for", r["name"], file=sys.stderr)
            continue
        rec["score"] = sum(rec[lang]["adjusted"] for lang in PRIMARY)
        rec["repo_score"] = sum(rec[lang]["repo_adjusted"] for lang in PRIMARY)
        rec["low"] = any(rec[lang]["low"] and rec[lang]["adjusted"] > 0.2 * rec["score"] for lang in PRIMARY)
        rec["raw"] = sum(rec[lang]["total"] for lang in PRIMARY)
        rec["se"] = se.get(r["name"])
        table.append(rec)
    pct = percentile_ranks([t["score"] for t in table])
    for t, p in zip(table, pct):
        t["percentile"] = p
    for t, p in zip(table, percentile_ranks([t["repo_score"] for t in table])):
        t["repo_percentile"] = p
    table.sort(key=lambda t: -t["score"])
    for i, t in enumerate(table):
        t["rank"] = i + 1

    with open(common.OUT / "usage.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["rank", "name", "group", "chapter", "implemented", "score", "percentile",
                    "sql_total", "sql_precision", "sql_adjusted", "plpgsql_total", "plpgsql_precision",
                    "plpgsql_adjusted", "python_total", "python_precision", "python_adjusted", "se_posts",
                    "skip_candidate", "repo_score", "repo_percentile", "sample_repos_sql", "sample_repos_plpgsql",
                    "low_confidence"])
        for t in table:
            py = t["Python"] or {}
            w.writerow([t["rank"], t["name"], t["group"], t["chapter"], "yes" if t["implemented"] else "",
                        round(t["score"]), round(t["percentile"], 1),
                        t["SQL"]["total"], round(t["SQL"]["precision"], 2), round(t["SQL"]["adjusted"]),
                        t["PLpgSQL"]["total"], round(t["PLpgSQL"]["precision"], 2), round(t["PLpgSQL"]["adjusted"]),
                        py.get("total", ""), round(py["precision"], 2) if py else "",
                        round(py["adjusted"]) if py else "", "" if t["se"] is None else t["se"],
                        "yes" if t["name"] in SKIP else "", round(t["repo_score"]), round(t["repo_percentile"], 1),
                        t["SQL"]["repos"], t["PLpgSQL"]["repos"], "yes" if t["low"] else ""])

    n = len(table)
    print(f"functions ranked: {n}")
    cutoff = sorted(t["score"] for t in table)[int(0.1 * n)]
    print(f"bottom-10% boundary score (value at index floor(0.1n)={int(0.1 * n)}): {cutoff:.1f}")

    se_rows = [t for t in table if t["se"] is not None]
    for t, p in zip(se_rows, percentile_ranks([t["se"] for t in se_rows])):
        t["se_percentile"] = p
    print("\n## Skip candidates")
    print("| Function | Rank | Score | Percentile | Bottom 10% | Repo-adjusted score | Repo percentile | SQL raw / precision / repos | PLpgSQL raw / precision / repos | SE posts (percentile in SE sample) |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for t in table:
        if t["name"] in SKIP:
            print(f"| {t['name']} | {t['rank']}/{n} | {t['score']:.0f} | {t['percentile']:.1f} | "
                  f"{'yes' if t['percentile'] < 10 else 'no'} | {t['repo_score']:.0f} | {t['repo_percentile']:.1f} | "
                  f"{t['SQL']['total']} / {t['SQL']['precision']:.2f} / {t['SQL']['repos']} | "
                  f"{t['PLpgSQL']['total']} / {t['PLpgSQL']['precision']:.2f} / {t['PLpgSQL']['repos']} | {t['se']} ({t.get('se_percentile', float('nan')):.0f}) |")
    print("\nlow-confidence rows:", ", ".join(t["name"] for t in table if t["low"]))

    # Validation.
    sub = [t for t in table if t["se"] is not None]
    rho, p = spearmanr([t["score"] for t in sub], [t["se"] for t in sub])
    print(f"\nSpearman(GitHub score, SE posts) on n={len(sub)}: rho={rho:.3f} p={p:.2g}")
    rho, p = spearmanr([t["SQL"]["adjusted"] for t in table], [t["PLpgSQL"]["adjusted"] for t in table])
    print(f"Spearman(SQL adjusted, PLpgSQL adjusted) n={n}: rho={rho:.3f}")
    rho, p = spearmanr([t["repo_score"] for t in sub], [t["se"] for t in sub])
    print(f"Spearman(repo-adjusted score, SE posts) on n={len(sub)}: rho={rho:.3f}")
    rho, p = spearmanr([t["score"] for t in table], [t["repo_score"] for t in table])
    print(f"Spearman(score, repo-adjusted score) n={n}: rho={rho:.3f}")
    rho, p = spearmanr([t["score"] for t in table], [t["raw"] for t in table])
    print(f"Spearman(score, raw total) n={n}: rho={rho:.3f}")
    py = [t for t in table if t["Python"]]
    if py:
        rho, p = spearmanr([t["score"] for t in py], [t["Python"]["adjusted"] for t in py])
        print(f"Spearman(score, Python adjusted) n={len(py)}: rho={rho:.3f}")
        se_py = [t for t in py if t["se"] is not None]
        rho, p = spearmanr([t["Python"]["adjusted"] for t in se_py], [t["se"] for t in se_py])
        print(f"Spearman(Python adjusted, SE posts) n={len(se_py)}: rho={rho:.3f}")
    # Agreement on the bottom decile.
    se_sorted = sorted(sub, key=lambda t: t["se"])
    k = max(1, int(0.1 * len(sub)))
    gh_bottom = {t["name"] for t in sorted(sub, key=lambda t: t["score"])[:k]}
    se_bottom = {t["name"] for t in se_sorted[:k]}
    print(f"bottom-{k} overlap GH vs SE in the SE sample: {len(gh_bottom & se_bottom)}/{k}")

    print("\n## Top unimplemented per group")
    groups: dict[str, list] = {}
    for t in table:
        if not t["implemented"]:
            groups.setdefault(t["group"], []).append(t)
    for g in sorted(groups):
        top = groups[g][:8]
        print(f"- {g}: " + ", ".join(f"{t['name']} ({t['score']:.0f}, p{t['percentile']:.0f})" for t in top))
    print("\n## Top 15 unimplemented overall")
    unimpl = [t for t in table if not t["implemented"]]
    for t in unimpl[:15]:
        print(f"| {t['rank']} | {t['name']} | {t['group']} | {t['score']:.0f} | {t['percentile']:.1f} |")

    print("\n## Classes summary (primary languages)")
    agg: dict[str, int] = {}
    for t in table:
        for lang in PRIMARY:
            for c, k2 in t[lang]["classes"].items():
                agg[c] = agg.get(c, 0) + k2
    print(agg)


if __name__ == "__main__":
    main()
