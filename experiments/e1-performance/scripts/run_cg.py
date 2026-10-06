#!/usr/bin/env python3
"""Instruction counts with cachegrind. Counts are deterministic to ~0.1%, so they can run in
parallel on a loaded machine. Appends TSV rows to results/cg.tsv:

    hyp  group  dataset  enc  rows  query  region  Ir

Usage: run_cg.py [--jobs N] [H1] [H1v] [H2] [H2v] [H6] [H6i]
"""

import argparse
import os
import re
import subprocess
from concurrent.futures import ThreadPoolExecutor

import configs

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.environ.get(
    "CARGO_TARGET_DIR", "/home/mikkel/Projects/geodatafusion/target/experiments/e1"
)
BIN = os.path.join(TARGET, "release", "e1-performance")
OUT = os.path.join(HERE, "..", "results", "cg.tsv")


def run(job):
    hyp, group, d, e, rows, q, region = job
    cmd = [
        "valgrind", "--tool=cachegrind", "--cache-sim=no", "--instr-at-start=no",
        "--cachegrind-out-file=/dev/null", BIN, "cg", region, d, e, str(rows), q,
    ]
    p = subprocess.run(cmd, capture_output=True, text=True)
    m = re.search(r"I\s+refs:\s+([\d,]+)", p.stderr)
    if p.returncode != 0 or not m:
        return f"# FAILED {job}: {p.stderr[-500:]!r}"
    ir = int(m.group(1).replace(",", ""))
    return "\t".join(map(str, [hyp, group, d, e, rows, q, region, ir]))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("hyps", nargs="*", default=["H1", "H2", "H6"])
    a = ap.parse_args()
    jobs = []
    h1_styles = ("t", "u") if "H1" in a.hyps else ("v",) if "H1v" in a.hyps else ()
    if h1_styles:
        for hyp, g, d, e, rows, qs in configs.h1_jobs(configs.CG_ROWS, h1_styles):
            for q in qs:
                for region in ("e2e", "kernel"):
                    jobs.append((hyp, g, d, e, rows, q, region))
    h2_styles = ("u", "t") if "H2" in a.hyps else ("v",) if "H2v" in a.hyps else ()
    if h2_styles:
        for hyp, g, d, e, rows, qs in configs.h2_jobs(configs.CG_ROWS, h2_styles):
            for q in qs:
                jobs.append((hyp, g, d, e, rows, q, "e2e"))
    if "H6" in a.hyps or "H6i" in a.hyps:
        for hyp, g, d, e, rows, qs in configs.h6_jobs(configs.CG_ROWS):
            for q in qs:
                # H6i: only the intersects variants added after the first run.
                if "H6" not in a.hyps and q not in ("u_intersectsrelate_aa", "e_intersects_aa"):
                    continue
                jobs.append((hyp, g, d, e, rows, q, "e2e"))
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with ThreadPoolExecutor(a.jobs) as ex, open(OUT, "a") as f:
        for i, line in enumerate(ex.map(run, jobs)):
            f.write(line + "\n")
            f.flush()
            print(f"[{i + 1}/{len(jobs)}] {line}", flush=True)


if __name__ == "__main__":
    main()
