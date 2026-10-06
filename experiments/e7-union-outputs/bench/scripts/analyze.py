#!/usr/bin/env python3
"""H2d tables. Ratios are WKB / union (> 1 means union outputs are faster).

Usage: analyze.py [wall tags...]      e.g. analyze.py wall1 wall2
Prints markdown: per-input raw tables (instructions per row, wall medians with min-max) and a
per-pipeline summary.
"""
import csv, os, statistics as st, sys
from collections import defaultdict

import configs

HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(HERE, "..", "..", "results")
P = list(configs.PIPELINES)
INPUTS = [(d, e) for d in configs.DATASETS for e in configs.ENCODINGS]


def load_cg():
    cg, sums = {}, defaultdict(set)
    for line in open(os.path.join(R, "h2d_cg.tsv")):
        if line.startswith("#"):
            print("<!-- ", line.strip(), " -->")
            continue
        p, d, e, rows, q, ir, s = line.rstrip("\n").split("\t")
        cg[(p, d, e, q.split("_")[1])] = int(ir) / int(rows)
        sums[(p, d, e)].add(s)
    bad = [k for k, v in sums.items() if len(v) != 1]
    return cg, bad


def load_wall(tag):
    w, sums = defaultdict(dict), defaultdict(set)
    for line in open(os.path.join(R, f"h2d_{tag}.tsv")):
        if line.startswith("#"):
            print("<!-- ", line.strip(), " -->")
            continue
        p, d, e, rows, q, rep, ms, s = line.rstrip("\n").split("\t")
        w[(p, d, e, q.split("_")[1])][int(rep)] = float(ms)
        sums[(p, d, e)].add(s)
    bad = [k for k, v in sums.items() if len(v) != 1]
    return w, bad


def ratio_reps(w, key_num, key_den):
    a, b = w[key_num], w[key_den]
    rs = [a[r] / b[r] for r in sorted(a) if r in b]
    return st.median(rs), min(rs), max(rs)


def best(vals):
    return min(vals)


def fmt_range(xs):
    return f"{min(xs):.2f}–{max(xs):.2f}"


def main():
    tags = sys.argv[1:]
    cg, bad = load_cg()
    print(f"checksum mismatches (cachegrind): {bad}\n")
    walls = {}
    for t in tags:
        walls[t], badw = load_wall(t)
        print(f"checksum mismatches ({t}): {badw}\n")

    # Summary per pipeline.
    print("### Summary: WKB / union, range over the 8 inputs (> 1: union faster)\n")
    hdr = "| pipeline | instr w/n | instr best/best |"
    for t in tags:
        hdr += f" {t} w/n | {t} best/best |"
    print(hdr)
    print("|---" * (3 + 2 * len(tags)) + "|")
    verdict = {}
    for p in P:
        row = f"| {p} |"
        rn = [cg[(p, d, e, "w")] / cg[(p, d, e, "n")] for d, e in INPUTS]
        rb = [min(cg[(p, d, e, "w")], cg[(p, d, e, "f")]) / min(cg[(p, d, e, "n")], cg[(p, d, e, "l")]) for d, e in INPUTS]
        row += f" {fmt_range(rn)} | {fmt_range(rb)} |"
        for t in tags:
            w = walls[t]
            wn = [ratio_reps(w, (p, d, e, "w"), (p, d, e, "n"))[0] for d, e in INPUTS]
            wb = []
            for d, e in INPUTS:
                med = {v: st.median(w[(p, d, e, v)].values()) for v in configs.VARIANTS}
                wb.append(min(med["w"], med["f"]) / min(med["n"], med["l"]))
            row += f" {fmt_range(wn)} | {fmt_range(wb)} |"
            verdict[(t, p)] = (wn, wb)
        print(row)
    print()
    for t in tags:
        for label, idx in [("w/n", 0), ("best/best", 1)]:
            for thr in (1.20, 1.25):
                any_ = [p for p in P if max(verdict[(t, p)][idx]) >= thr]
                all_ = [p for p in P if min(verdict[(t, p)][idx]) >= thr]
                print(f"- {t} {label} ≥ {thr}: on some input: {any_ or 'none'}; on every input: {all_ or 'none'}")
    print()

    # Raw tables.
    print("### Instructions per row (cachegrind, end to end)\n")
    print("| pipeline | dataset | enc | rows | n | l | w | f | w/n | best/best |")
    print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|")
    rows_of = {(p, d): configs.CG_ROWS[configs.PIPELINES[p]][d] for p in P for d in configs.DATASETS}
    for p in P:
        for d, e in INPUTS:
            v = {x: cg[(p, d, e, x)] for x in configs.VARIANTS}
            print(f"| {p} | {d} | {e} | {rows_of[(p, d)]:,} | " + " | ".join(f"{v[x]:,.0f}" for x in configs.VARIANTS)
                  + f" | {v['w'] / v['n']:.2f} | {min(v['w'], v['f']) / min(v['n'], v['l']):.2f} |")
    print()
    for t in tags:
        w = walls[t]
        print(f"### Wall clock ({t}): median ms (min–max), {len(next(iter(w.values())))} repetitions\n")
        print("| pipeline | dataset | enc | rows | n | l | w | f | w/n median (min–max of per-rep ratios) |")
        print("|---|---|---|--:|--:|--:|--:|--:|--:|")
        wrows = {(p, d): configs.WALL_ROWS[configs.PIPELINES[p]][d] for p in P for d in configs.DATASETS}
        for p in P:
            for d, e in INPUTS:
                cells = []
                for x in configs.VARIANTS:
                    vals = list(w[(p, d, e, x)].values())
                    cells.append(f"{st.median(vals):.1f} ({min(vals):.1f}–{max(vals):.1f})")
                m, lo, hi = ratio_reps(w, (p, d, e, "w"), (p, d, e, "n"))
                print(f"| {p} | {d} | {e} | {wrows[(p, d)]:,} | " + " | ".join(cells) + f" | {m:.2f} ({lo:.2f}–{hi:.2f}) |")
        print()


if __name__ == "__main__":
    main()
