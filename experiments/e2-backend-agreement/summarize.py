"""Print the E2 result tables (markdown) from out/per_function/*.json(l).

Usage: python3 summarize.py [--examples]
"""

import collections
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA = Path(os.environ.get("E2_DATA", REPO / "target/experiments/e2"))
PF = DATA / "out/per_function"
S = {p.stem: json.loads(p.read_text()) for p in PF.glob("*.json")}
BACKENDS = ["geo_raw", "geo_norm", "geos_raw", "geos_wrap"]
ORDER = [
    "st_isvalid", "st_pointonsurface", "st_convexhull", "st_orientedenvelope", "st_simplify",
    "st_simplifyvw", "st_centroid", "st_area", "st_length", "st_distance", "st_contains",
    "st_intersects", "st_within", "st_touches", "st_relate",
]


def pct(a, n):
    return f"{100 * a / n:.2f}%" if n else "-"


def cell(st):
    return f"{st['agree']}/{st['total']} ({pct(st['agree'], st['total'])})"


print("## Agreement\n")
print("| Function | n | geo raw | geo normalized | GEOS raw | GEOS + PostGIS rules | geo normalized, valid non-empty inputs |")
print("|---|---|---|---|---|---|---|")
for f in ORDER:
    if f not in S:
        continue
    s = S[f]
    gn = s["geo_norm"]
    vn = gn["total_valid_nonempty"]
    print(f"| {f} | {s['geo_raw']['total']} | " + " | ".join(cell(s[b]) for b in BACKENDS)
          + f" | {vn - gn['disagree_valid_nonempty']}/{vn} ({pct(vn - gn['disagree_valid_nonempty'], vn)}) |")

rows = [json.loads(line) for p in sorted(PF.glob("*.jsonl")) for line in open(p)]
print("\n## Disagreement kinds (E = an input is or contains EMPTY, I = an input is invalid in PostGIS, V = valid non-empty)\n")
for b in BACKENDS:
    print(f"\n### {b}\n")
    print("| Function | kind | E | I | V |")
    print("|---|---|---|---|---|")
    for f in ORDER:
        rs = [r for r in rows if r["func"] == f and r["backend"] == b]
        c = collections.Counter((r["kind"], "E" if r["empty_input"] else ("I" if r["invalid_input"] else "V")) for r in rs)
        for kind in sorted({k for k, _ in c}, key=lambda k: -sum(v for (kk, _), v in c.items() if kk == k)):
            print(f"| {f} | {kind} | {c[(kind, 'E')]} | {c[(kind, 'I')]} | {c[(kind, 'V')]} |")

print("\n## Disagreements by corpus source (geo normalized)\n")
src_tot = collections.Counter()
for f in ORDER:
    for src, (n, d) in S[f]["geo_norm"]["by_src"].items():
        src_tot[(f, src)] = d
for f in ORDER:
    top = sorted(((d, src) for (ff, src), d in src_tot.items() if ff == f and d), reverse=True)[:5]
    print(f"- {f}: " + ", ".join(f"{src} {d}" for d, src in top))

if "--examples" in sys.argv:
    print("\n## Smallest examples per (function, backend, kind)\n")
    groups = collections.defaultdict(list)
    for r in rows:
        groups[(r["func"], r["backend"], r["kind"])].append(r)
    for k in sorted(groups):
        rs = sorted(groups[k], key=lambda r: r["npoints"])[:2]
        for r in rs:
            print(f"{k} id={r['id']} src={r['src']} E={r['empty_input']} I={r['invalid_input']} geos_agrees={r['geos_agrees']}")
            print("  in :", r["input"][:300].replace("\n", " | "))
            print("  pg :", r["pg"][:300])
            print("  got:", r["got"][:300])
