#!/usr/bin/env python3
"""Wall clock: one process per (pipeline, input); the four variants interleaved in-process with
a rotating order, 1 warm-up + REPS measured repetitions. Appends to results/<tag>.tsv:

    pipeline  dataset  enc  rows  query  rep  wall_ms  checksum

Usage: run_wall.py --tag wall1 [--reps 7]
"""
import argparse, os, subprocess

import configs
from run_cg import BIN, RESULTS


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", default="wall1")
    ap.add_argument("--reps", type=int, default=7)
    a = ap.parse_args()
    jobs = list(configs.jobs(configs.WALL_ROWS))
    out = os.path.join(RESULTS, f"h2d_{a.tag}.tsv")
    with open(out, "a") as f:
        for i, (p, d, e, rows, qs) in enumerate(jobs):
            cmd = [BIN, "run", d, e, str(rows), str(a.reps), *qs]
            r = subprocess.run(cmd, capture_output=True, text=True)
            if r.returncode != 0:
                f.write(f"# FAILED {cmd}: {r.stderr[-500:]!r}\n")
                continue
            for line in r.stdout.strip().splitlines():
                f.write(f"{p}\t{line}\n")
            f.flush()
            print(f"[{i + 1}/{len(jobs)}] {p} {d} {e} {rows} done", flush=True)


if __name__ == "__main__":
    main()
