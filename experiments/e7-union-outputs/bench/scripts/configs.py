"""The E7 H2d run matrix (E1's inputs and row counts)."""

DATASETS = ["points", "poly10", "poly100", "poly1000"]
ENCODINGS = ["sep", "wkb"]
VARIANTS = ["n", "l", "w", "f"]  # union GeometryBuilder, union local, WKB WkbBuilder, WKB fast

# E1's rows per cost class: "mid" for p1-p3 (and the p5 chain), "buffer" for p4.
WALL_ROWS = {
    "mid": {"points": 1_000_000, "poly10": 100_000, "poly100": 100_000, "poly1000": 10_000},
    "buffer": {"points": 100_000, "poly10": 10_000, "poly100": 2_000, "poly1000": 200},
}
CG_ROWS = {
    "mid": {"points": 100_000, "poly10": 10_000, "poly100": 10_000, "poly1000": 1_000},
    "buffer": {"points": 10_000, "poly10": 1_000, "poly100": 200, "poly1000": 50},
}
PIPELINES = {"p1": "mid", "p2": "mid", "p3": "mid", "p4": "buffer", "p5": "mid"}


def jobs(rows_table):
    for p, cls in PIPELINES.items():
        for d in DATASETS:
            for e in ENCODINGS:
                yield (p, d, e, rows_table[cls][d], [f"{p}_{v}" for v in VARIANTS])
