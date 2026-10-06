#!/usr/bin/env python3
"""Side-by-side H2e table from results/h2e_df54.tsv and results/h2e_df55.tsv."""
import csv, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(HERE, "..", "results")

def load(name):
    with open(os.path.join(R, name)) as f:
        rows = list(csv.DictReader(f, delimiter="\t"))
    return {(r["kind"], r["source"], r["construct"]): r for r in rows}

def cell(r):
    if r is None:
        return "n/a"
    k = r["keeps"]
    if r["status"].startswith("ERROR"):
        return "error: " + r["detail"].split("=> ", 1)[-1][:90]
    if "exec ERROR" in r["status"]:
        tag = "tag kept in plan" if r["out_field"] != "-" else "no tag in plan"
        return f"{k}; {tag}; exec error"
    return k

a, b = load("h2e_df54.tsv"), load("h2e_df55.tsv")
keys = list(dict.fromkeys(list(a) + list(b)))
print("| kind | source | construct | DataFusion 54 | DataFusion 55 |")
print("|---|---|---|---|---|")
for key in keys:
    print(f"| {key[0]} | {key[1]} | {key[2]} | {cell(a.get(key))} | {cell(b.get(key))} |")
