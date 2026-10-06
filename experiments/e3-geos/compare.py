#!/usr/bin/env python3
"""Compare E3 result files pairwise, under the harness rules (canonical column) and bit-exactly (raw).

Usage: compare.py <dir>   (expects corpus.tsv, geos-3.12.tsv, geos-3.14.tsv, geos-3.15.tsv,
postgis-3.14.tsv in <dir>; writes <dir>/diffs-<a>-vs-<b>.tsv and prints markdown tables)
"""

import collections
import itertools
import re
import sys

NUM = re.compile(r"-?\d+(?:\.\d+)?(?:e[-+]?\d+)?|NaN|-?Infinity")
SETS = ["geos-3.12", "geos-3.14", "geos-3.15", "postgis-3.14"]


def load(path):
    rows = {}
    with open(path) as f:
        next(f)
        for line in f:
            id_, op, raw, canon = line.rstrip("\n").split("\t", 3)
            rows[(int(id_), op)] = (raw, canon)
    return rows


def kinds(path):
    out = {}
    with open(path) as f:
        next(f)
        for line in f:
            p = line.split("\t", 2)
            out[int(p[0])] = p[1]
    return out


def gtype(c):
    m = re.match(r"(SRID=\d+;)?([A-Z]+)", c)
    return m.group(2) if m else c


def nums(c):
    return [float(x) for x in NUM.findall(re.sub(r"^SRID=\d+;", "", c))]


def parse(c):
    """Parse canonical WKT into (type, nested lists); coordinates as tuples of floats."""
    c = re.sub(r"^SRID=\d+;", "", c)
    toks = re.findall(r"[A-Z]+|\(|\)|,|[^\s(),]+(?: [^\s(),]+)*", c)
    pos = 0

    def node():
        nonlocal pos
        t = toks[pos]
        if t == "(":
            pos += 1
            items = []
            while toks[pos] != ")":
                if toks[pos] == ",":
                    pos += 1
                    continue
                items.append(node())
            pos += 1
            return items
        if re.match(r"[A-Z]+$", t):
            pos += 1
            while pos < len(toks) and re.match(r"[A-Z]+$", toks[pos]) and toks[pos] not in ("EMPTY",):
                pos += 1  # Z/M tags
            if pos < len(toks) and toks[pos] == "EMPTY":
                pos += 1
                return (t, [])
            return (t, node())
        pos += 1
        return tuple(float(x) for x in t.split())

    return node()


def norm_ring(r):
    r = r[:-1] if len(r) > 1 and r[0] == r[-1] else r
    best = None
    for seq in (r, r[::-1]):
        i = seq.index(min(seq))
        cand = tuple(seq[i:] + seq[:i])
        best = cand if best is None or cand < best else best
    return best


def norm(g):
    """Geometry up to ring start vertex, ring orientation, hole order and part order."""
    t, body = g
    if t == "POLYGON":
        return (t, (norm_ring(body[0]),) + tuple(sorted(norm_ring(h) for h in body[1:])) if body else ())
    if t == "MULTIPOLYGON":
        return (t, tuple(sorted(norm(("POLYGON", p)) for p in body)))
    if t in ("MULTILINESTRING",):
        return (t, tuple(sorted(min(tuple(l), tuple(l[::-1])) for l in body)))
    if t == "LINESTRING":
        return (t, min(tuple(body), tuple(body[::-1])))
    if t == "MULTIPOINT":
        return (t, tuple(sorted(tuple(p[0]) if isinstance(p, list) else p for p in body)))
    if t == "GEOMETRYCOLLECTION":
        return (t, tuple(sorted((norm(x) for x in body), key=repr)))
    return (t, repr(body))


def category(ra, ca, rb, cb):
    ea, eb = ra.startswith("E:"), rb.startswith("E:")
    if ea != eb:
        return "error vs result"
    if ea and eb:
        return "both errors"  # canonical equal; unreachable for canonical diffs
    if ra.startswith("T:") or rb.startswith("T:"):
        if ra.split("[")[0] != rb.split("[")[0]:
            return "validity reason text differs"
        return "validity reason location differs"
    if gtype(ca) != gtype(cb):
        return "geometry type differs"
    try:
        if norm(parse(ca)) == norm(parse(cb)):
            return "same geometry up to ring start/orientation/part order"
    except Exception as e:  # noqa: BLE001
        print("parse failure", e, ca[:80], file=sys.stderr)
    na, nb = nums(ca), nums(cb)
    if len(na) != len(nb):
        return "vertex count differs"
    rel = max(abs(x - y) / max(abs(x), abs(y), 1e-300) for x, y in zip(na, nb))
    if rel < 1e-9:
        return "same vertex count, coords differ < 1e-9 rel"
    return "same vertex count, coords differ"


def main():
    d = sys.argv[1]
    data = {s: load(f"{d}/{s}.tsv") for s in SETS}
    kind = kinds(f"{d}/corpus.tsv")
    keys = sorted(data[SETS[0]])
    for s in SETS:
        assert sorted(data[s]) == keys, s
    ops = sorted({op for _, op in keys})
    print(f"results per set: {len(keys)}\n")

    # Pairwise totals.
    print("| pair | raw (bit-exact) differences | canonical (harness) differences |")
    print("|---|---|---|")
    pair_diffs = {}
    for a, b in itertools.combinations(SETS, 2):
        raw = [k for k in keys if data[a][k][0] != data[b][k][0]]
        can = [k for k in keys if data[a][k][1] != data[b][k][1]]
        pair_diffs[(a, b)] = can
        print(f"| {a} vs {b} | {len(raw)} ({100 * len(raw) / len(keys):.2f}%) | {len(can)} ({100 * len(can) / len(keys):.2f}%) |")
        with open(f"{d}/diffs-{a}-vs-{b}.tsv", "w") as f:
            f.write("id\top\tkind\tcategory\t" + a + "\t" + b + "\n")
            for k in can:
                (ra, ca), (rb, cb) = data[a][k], data[b][k]
                f.write(f"{k[0]}\t{k[1]}\t{kind[k[0]]}\t{category(ra, ca, rb, cb)}\t{ca if not ra.startswith('E:') else ra}\t{cb if not rb.startswith('E:') else rb}\n")
    print()

    # Per op, canonical differences for each pair.
    pairs = list(itertools.combinations(SETS, 2))
    print("| op | n | " + " | ".join(f"{a[5:] if a.startswith('geos') else 'PG'} vs {b[5:] if b.startswith('geos') else 'PG'}" for a, b in pairs) + " |")
    print("|---|---|" + "---|" * len(pairs))
    for op in ops:
        n = sum(1 for k in keys if k[1] == op)
        cells = [str(sum(1 for k in pair_diffs[p] if k[1] == op)) for p in pairs]
        print(f"| {op} | {n} | " + " | ".join(cells) + " |")
    print()

    # Categories per pair.
    for p in pairs:
        if not pair_diffs[p]:
            continue
        print(f"**{p[0]} vs {p[1]}** categories (op: category: count)\n")
        c = collections.Counter()
        for k in pair_diffs[p]:
            (ra, ca), (rb, cb) = data[p[0]][k], data[p[1]][k]
            c[(k[1], category(ra, ca, rb, cb))] += 1
        for (op, cat), n in sorted(c.items()):
            print(f"- {op}: {cat}: {n}")
        ck = collections.Counter(kind[k[0]] for k in pair_diffs[p])
        print("- by corpus kind: " + ", ".join(f"{k} {n}" for k, n in ck.most_common()))
        print()


if __name__ == "__main__":
    main()
