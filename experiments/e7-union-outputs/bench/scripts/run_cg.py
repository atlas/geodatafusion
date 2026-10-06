#!/usr/bin/env python3
"""Instruction counts (cachegrind, end-to-end region). Appends to results/h2d_cg.tsv:

    pipeline  dataset  enc  rows  query  Ir  checksum

Usage: run_cg.py [--jobs N]
"""
import argparse, os, re, subprocess
from concurrent.futures import ThreadPoolExecutor

import configs

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.environ.get("CARGO_TARGET_DIR", "/home/mikkel/Projects/geodatafusion/target/experiments/e7")
BIN = os.path.join(TARGET, "release", "e7-bench")
RESULTS = os.path.join(HERE, "..", "..", "results")


def run(job):
    p, d, e, rows, q = job
    cmd = ["valgrind", "--tool=cachegrind", "--cache-sim=no", "--instr-at-start=no",
           "--cachegrind-out-file=/dev/null", BIN, "cg", d, e, str(rows), q]
    r = subprocess.run(cmd, capture_output=True, text=True)
    m = re.search(r"I\s+refs:\s+([\d,]+)", r.stderr)
    c = re.search(r"checksum (\S+)", r.stderr)
    if r.returncode != 0 or not m:
        return f"# FAILED {job}: {r.stderr[-500:]!r}"
    return "\t".join(map(str, [p, d, e, rows, q, int(m.group(1).replace(",", "")), c.group(1) if c else ""]))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--out", default="h2d_cg.tsv")
    a = ap.parse_args()
    jobs = [(p, d, e, rows, q) for p, d, e, rows, qs in configs.jobs(configs.CG_ROWS) for q in qs]
    out = os.path.join(RESULTS, a.out)
    with ThreadPoolExecutor(a.jobs) as ex, open(out, "a") as f:
        for i, line in enumerate(ex.map(run, jobs)):
            f.write(line + "\n")
            f.flush()
            print(f"[{i + 1}/{len(jobs)}] {line}", flush=True)


if __name__ == "__main__":
    main()
