#!/usr/bin/env python3
"""Wall-clock runs, one process per (input, query group), variants interleaved in-process with a
rotating order, 1 warm-up + REPS measured repetitions. Appends TSV rows to results/<tag>.tsv:

    hyp  group  dataset  enc  rows  query  rep  wall_ms  kernel_ms  checksum

Usage: run_wall.py --tag wall1 [--reps 5] [H1] [H2] [H6]
"""

import argparse
import os
import subprocess

import configs
from run_cg import BIN, HERE


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", default="wall1")
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("hyps", nargs="*", default=["H1", "H2", "H6"])
    a = ap.parse_args()
    jobs = []
    if "H1" in a.hyps:
        jobs += list(configs.h1_jobs(configs.WALL_ROWS))
    if "H2" in a.hyps:
        jobs += list(configs.h2_jobs(configs.WALL_ROWS))
    if "H6" in a.hyps:
        jobs += list(configs.h6_jobs(configs.WALL_ROWS))
    out = os.path.join(HERE, "..", "results", f"{a.tag}.tsv")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "a") as f:
        for i, (hyp, g, d, e, rows, qs) in enumerate(jobs):
            cmd = [BIN, "run", d, e, str(rows), str(a.reps), *qs]
            p = subprocess.run(cmd, capture_output=True, text=True)
            if p.returncode != 0:
                f.write(f"# FAILED {cmd}: {p.stderr[-500:]!r}\n")
                continue
            for line in p.stdout.strip().splitlines():
                f.write(f"{hyp}\t{g}\t{line}\n")
            f.flush()
            print(f"[{i + 1}/{len(jobs)}] {hyp} {g} {d} {e} {rows} done", flush=True)


if __name__ == "__main__":
    main()
