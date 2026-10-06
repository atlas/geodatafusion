#!/usr/bin/env python3
"""Turns results/cg.tsv and results/<wall>.tsv into the markdown tables of
plans/experiments/e1-performance.md.

Usage: analyze.py [wall_tag ...]
"""

import os
import statistics
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
RES = os.path.join(HERE, "..", "results")

DS = ["points", "poly10", "poly100", "poly1000"]
ENC = ["sep", "int", "wkb"]


def load_cg():
    cg = {}
    for line in open(os.path.join(RES, "cg.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        hyp, g, d, e, rows, q, region, ir = line.rstrip("\n").split("\t")
        cg[(hyp, g, d, e, q, region)] = (int(ir), int(rows))
    return cg


def load_wall(tag):
    w = defaultdict(list)
    for line in open(os.path.join(RES, f"{tag}.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        hyp, g, d, e, rows, q, rep, wall, kernel, cs = line.rstrip("\n").split("\t")
        w[(hyp, g, d, e, q)].append((int(rep), float(wall), float(kernel), cs, int(rows)))
    return w


def per_row(v):
    ir, rows = v
    return ir / rows


def fmt_ir(x):
    return f"{x:,.0f}" if x >= 100 else f"{x:.1f}"


def fmt_ratio(r):
    return f"{r:.2f}"


def paired(w, key_a, key_b, idx):
    """Median and min-max of per-rep ratios b/a (the reps were interleaved), plus medians."""
    a = {rep: v[idx] for rep, *v in [(x[0], x[1], x[2]) for x in w[key_a]]}
    b = {rep: v[idx] for rep, *v in [(x[0], x[1], x[2]) for x in w[key_b]]}
    reps = sorted(set(a) & set(b))
    ratios = [b[r] / a[r] for r in reps if a[r] > 0]
    if not ratios:
        return None
    return (
        statistics.median(a[r] for r in reps),
        statistics.median(b[r] for r in reps),
        statistics.median(ratios),
        min(ratios),
        max(ratios),
    )


def h1(cg, walls):
    print("### H1: instruction counts per row (cachegrind)\n")
    print("Overhead = unified / typed. `e2e` is the whole query (planning, MemTable scan, "
          "kernels, collect), `kernel` is `invoke_with_args` only.\n")
    print("| function | input | enc | rows | typed e2e | unified e2e | **e2e ratio** | "
          "typed kernel | unified kernel | kernel ratio | fast-WKB e2e | fast-WKB ratio |")
    print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|")
    for f in ["x", "npoints", "isempty", "area", "centroid", "intersects", "buffer"]:
        for d in DS:
            for e in ENC:
                k = lambda q, r: cg.get(("H1", f, d, e, q, r))
                te, ue, tk, uk = k(f"t_{f}", "e2e"), k(f"u_{f}", "e2e"), k(f"t_{f}", "kernel"), k(f"u_{f}", "kernel")
                if not te or not ue:
                    continue
                cols = [f, d, e, f"{te[1]:,}", fmt_ir(per_row(te)), fmt_ir(per_row(ue)),
                        f"**{fmt_ratio(ue[0] / te[0])}**"]
                if tk and uk:
                    cols += [fmt_ir(per_row(tk)), fmt_ir(per_row(uk)), fmt_ratio(uk[0] / tk[0])]
                else:
                    cols += ["", "", ""]
                ve = k(f"v_{f}", "e2e")
                cols += [fmt_ir(per_row(ve)), fmt_ratio(ve[0] / te[0])] if ve else ["", ""]
                print("| " + " | ".join(cols) + " |")
    print()
    for tag, w in walls:
        print(f"### H1: wall clock ({tag})\n")
        print("Median of 5 interleaved repetitions; ratio = median of the per-repetition "
              "unified/typed ratios, with min–max.\n")
        print("| function | input | enc | rows | typed ms | unified ms | **e2e ratio** (min–max) "
              "| typed kernel ms | unified kernel ms | kernel ratio | fast-WKB ratio |")
        print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|")
        for f in ["x", "npoints", "isempty", "area", "centroid", "intersects", "buffer"]:
            for d in DS:
                for e in ENC:
                    a, b = ("H1", f, d, e, f"t_{f}"), ("H1", f, d, e, f"u_{f}")
                    if a not in w:
                        continue
                    pe = paired(w, a, b, 0)
                    pk = paired(w, a, b, 1)
                    rows = w[a][0][4]
                    pv = paired(w, a, ("H1", f, d, e, f"v_{f}"), 0) if ("H1", f, d, e, f"v_{f}") in w else None
                    vs = f"{pv[2]:.2f}" if pv else ""
                    print(f"| {f} | {d} | {e} | {rows:,} | {pe[0]:.1f} | {pe[1]:.1f} | "
                          f"**{pe[2]:.2f}** ({pe[3]:.2f}–{pe[4]:.2f}) | {pk[0]:.1f} | {pk[1]:.1f} | {pk[2]:.2f} | {vs} |")
        print()


def h2(cg, walls):
    print("### H2: instruction counts per row, end to end (cachegrind)\n")
    print("Ratio = WKB-output / native-output. Above 1.25 means native is ≥ 20% faster "
          "(time reduction of 20%).\n")
    print("| pipeline | input | enc | rows | unified native | unified WKB | **unified W/N** "
          "| typed native | typed WKB | typed W/N | fast-WKB unified W/N |")
    print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|")
    for p in ["p1", "p2", "p3", "p4"]:
        for d in DS:
            for e in ENC:
                k = lambda q: cg.get(("H2", p, d, e, q, "e2e"))
                un, uw, tn, tw = k(f"h2_{p}_u_n"), k(f"h2_{p}_u_w"), k(f"h2_{p}_t_n"), k(f"h2_{p}_t_w")
                if not un or not uw:
                    continue
                cols = [p, d, e, f"{un[1]:,}", fmt_ir(per_row(un)), fmt_ir(per_row(uw)),
                        f"**{uw[0] / un[0]:.2f}**"]
                cols += [fmt_ir(per_row(tn)), fmt_ir(per_row(tw)), f"{tw[0] / tn[0]:.2f}"] if tn and tw else ["", "", ""]
                vn, vw = k(f"h2_{p}_v_n"), k(f"h2_{p}_v_w")
                cols += [f"{vw[0] / vn[0]:.2f}"] if vn and vw else [""]
                print("| " + " | ".join(cols) + " |")
    print()
    for tag, w in walls:
        print(f"### H2: wall clock ({tag})\n")
        print("| pipeline | input | enc | rows | unified native ms | unified WKB ms | **unified W/N** (min–max) "
              "| typed native ms | typed WKB ms | typed W/N (min–max) |")
        print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|")
        for p in ["p1", "p2", "p3", "p4"]:
            for d in DS:
                for e in ENC:
                    a, b = ("H2", p, d, e, f"h2_{p}_u_n"), ("H2", p, d, e, f"h2_{p}_u_w")
                    if a not in w:
                        continue
                    pu = paired(w, a, b, 0)
                    pt = paired(w, ("H2", p, d, e, f"h2_{p}_t_n"), ("H2", p, d, e, f"h2_{p}_t_w"), 0)
                    rows = w[a][0][4]
                    print(f"| {p} | {d} | {e} | {rows:,} | {pu[0]:.1f} | {pu[1]:.1f} | "
                          f"**{pu[2]:.2f}** ({pu[3]:.2f}–{pu[4]:.2f}) | {pt[0]:.1f} | {pt[1]:.1f} | "
                          f"{pt[2]:.2f} ({pt[3]:.2f}–{pt[4]:.2f}) |")
        print()


def h6(cg, walls):
    print("### H6: instruction counts per row, end to end (cachegrind)\n")
    print("`expr-geo` = geodatafusion's current UDF (calls geoarrow-expr-geo). Ratio = owned / "
          "expr-geo; ≤ 1.10 is within 10%.\n")
    print("| function | input | enc | rows | expr-geo | owned | **owned / expr-geo** | owned (same type) | same / expr-geo |")
    print("|---|---|---|--:|--:|--:|--:|--:|--:|")
    for f, (g, u) in [("area", ("g_area", "u_area")), ("centroid", ("g_centroid", "u_centroid")),
                      ("simplify", ("g_simplify", "u_simplify")),
                      ("intersects", ("g_intersects_aa", "u_intersectsrelate_aa")),
                      ("intersects", ("e_intersects_aa", "u_intersects_aa")),
                      ("intersects_const", ("g_intersects", "u_intersects"))]:
        for d in DS:
            for e in ENC:
                a, b = cg.get(("H6", f, d, e, g, "e2e")), cg.get(("H6", f, d, e, u, "e2e"))
                f_label = f + (" (relate)" if g == "g_intersects_aa" else " (trait)" if g == "e_intersects_aa" else "")
                if not a or not b:
                    continue
                cols = [f_label, d, e, f"{a[1]:,}", fmt_ir(per_row(a)), fmt_ir(per_row(b)), f"**{b[0] / a[0]:.2f}**"]
                s = cg.get(("H6", f, d, e, "u_simplify_same", "e2e")) if f == "simplify" else None
                cols += [fmt_ir(per_row(s)), f"{s[0] / a[0]:.2f}"] if s else ["", ""]
                print("| " + " | ".join(cols) + " |")
    print()
    print("### H6: typed owned kernels vs expr-geo (cachegrind, e2e per row)\n")
    print("The H1 typed variants (`t_area`, `t_centroid`: owned kernel, `downcast_geoarrow_array!` "
          "loop, same rows) against today's expr-geo UDFs.\n")
    print("| function | input | enc | rows | expr-geo | owned typed | **ratio** |")
    print("|---|---|---|--:|--:|--:|--:|")
    for f in ["area", "centroid"]:
        for d in DS:
            for e in ENC:
                a = cg.get(("H6", f, d, e, f"g_{f}", "e2e"))
                b = cg.get(("H1", f, d, e, f"t_{f}", "e2e"))
                if a and b and a[1] == b[1]:
                    print(f"| {f} | {d} | {e} | {a[1]:,} | {fmt_ir(per_row(a))} | {fmt_ir(per_row(b))} | **{b[0] / a[0]:.2f}** |")
    print()
    for tag, w in walls:
        print(f"### H6: wall clock ({tag})\n")
        print("| function | input | enc | rows | expr-geo ms | owned ms | **owned / expr-geo** (min–max) | same / expr-geo |")
        print("|---|---|---|--:|--:|--:|--:|--:|")
        for f, (g, u) in [("area", ("g_area", "u_area")), ("centroid", ("g_centroid", "u_centroid")),
                          ("simplify", ("g_simplify", "u_simplify")),
                          ("intersects", ("g_intersects_aa", "u_intersectsrelate_aa")),
                          ("intersects", ("e_intersects_aa", "u_intersects_aa")),
                          ("intersects_const", ("g_intersects", "u_intersects"))]:
            for d in DS:
                for e in ENC:
                    a, b = ("H6", f, d, e, g), ("H6", f, d, e, u)
                    if a not in w or b not in w:
                        continue
                    f_label = f + (" (relate)" if g == "g_intersects_aa" else " (trait)" if g == "e_intersects_aa" else "")
                    p = paired(w, a, b, 0)
                    same = ""
                    if f == "simplify":
                        s = paired(w, a, ("H6", f, d, e, "u_simplify_same"), 0)
                        same = f"{s[2]:.2f}"
                    rows = w[a][0][4]
                    print(f"| {f_label} | {d} | {e} | {rows:,} | {p[0]:.1f} | {p[1]:.1f} | "
                          f"**{p[2]:.2f}** ({p[3]:.2f}–{p[4]:.2f}) | {same} |")
        print()


def checksums(walls):
    bad = []
    for tag, w in walls:
        groups = defaultdict(set)
        for (hyp, g, d, e, q), v in w.items():
            for _, _, _, cs, _ in v:
                groups[(hyp, g, d, e)].add((q, cs))
        for k, s in groups.items():
            sums = {cs for q, cs in s if not cs.endswith(":0.000000e0") or True}
            if len({cs for _, cs in s}) > 1:
                bad.append((tag, k, sorted(s)))
    print(f"<!-- checksum groups with differing results: {len(bad)} -->")
    for b in bad:
        print(f"<!-- {b} -->")


def main():
    cg = load_cg()
    walls = [(t, load_wall(t)) for t in sys.argv[1:]]
    which = os.environ.get("ONLY", "H1,H2,H6").split(",")
    if "H1" in which:
        h1(cg, walls)
    if "H2" in which:
        h2(cg, walls)
    if "H6" in which:
        h6(cg, walls)
    checksums(walls)


if __name__ == "__main__" and not os.environ.get("SUMMARY"):
    main()


def summary(cg, walls):
    """Min–max ratio per function, split into native-encoded and WKB inputs (for the verdicts)."""
    def rng(xs):
        return f"{min(xs):.2f}–{max(xs):.2f}" if xs else ""

    print("| function | cg native | cg WKB | " + " | ".join(f"{t} native | {t} WKB" for t, _ in walls) + " |")
    print("|---|--:|--:|" + "--:|--:|" * len(walls))
    for f in ["x", "npoints", "isempty", "area", "centroid", "intersects", "buffer"]:
        cols = [f]
        for enc_set in (("sep", "int"), ("wkb",)):
            xs = [cg[("H1", f, d, e, f"u_{f}", "e2e")][0] / cg[("H1", f, d, e, f"t_{f}", "e2e")][0]
                  for d in DS for e in enc_set if ("H1", f, d, e, f"t_{f}", "e2e") in cg]
            cols.append(rng(xs))
        for _, w in walls:
            for enc_set in (("sep", "int"), ("wkb",)):
                xs = [paired(w, ("H1", f, d, e, f"t_{f}"), ("H1", f, d, e, f"u_{f}"), 0)[2]
                      for d in DS for e in enc_set if ("H1", f, d, e, f"t_{f}") in w]
                cols.append(rng(xs))
        print("| " + " | ".join(cols) + " |")


if __name__ == "__main__" and os.environ.get("SUMMARY"):
    summary(load_cg(), [(t, load_wall(t)) for t in sys.argv[1:]])
