"""The E1 run matrix: which queries run on which inputs, at how many rows."""

DATASETS = ["points", "poly10", "poly100", "poly1000"]
ENCODINGS = ["sep", "int", "wkb"]

# Rows per (cost class, dataset). Wall-clock runs use the pre-registered sizes (1M points, 100k
# polygons) where a run stays within a few seconds, and fewer rows for the expensive functions.
# Cachegrind runs use fewer rows again (valgrind is ~50x slower); results are reported per row.
WALL_ROWS = {
    "cheap": {"points": 1_000_000, "poly10": 100_000, "poly100": 100_000, "poly1000": 100_000},
    "mid": {"points": 1_000_000, "poly10": 100_000, "poly100": 100_000, "poly1000": 10_000},
    "intersects": {"points": 1_000_000, "poly10": 100_000, "poly100": 20_000, "poly1000": 2_000},
    "buffer": {"points": 100_000, "poly10": 10_000, "poly100": 2_000, "poly1000": 200},
}
CG_ROWS = {
    "cheap": {"points": 100_000, "poly10": 10_000, "poly100": 10_000, "poly1000": 10_000},
    "mid": {"points": 100_000, "poly10": 10_000, "poly100": 10_000, "poly1000": 1_000},
    "intersects": {"points": 100_000, "poly10": 10_000, "poly100": 2_000, "poly1000": 500},
    "buffer": {"points": 10_000, "poly10": 1_000, "poly100": 200, "poly1000": 50},
}

H1_FUNCS = {
    "x": "cheap",
    "npoints": "cheap",
    "isempty": "cheap",
    "area": "mid",
    "centroid": "mid",
    "intersects": "intersects",
    "buffer": "buffer",
}

H2_PIPELINES = {"p1": "mid", "p2": "mid", "p3": "mid", "p4": "buffer"}

# H6: geodatafusion today (g_, which calls geoarrow-expr-geo) vs owned GeoColumn kernels (u_).
H6_QUERIES = {
    "area": (["g_area", "u_area"], "mid"),
    "centroid": (["g_centroid", "u_centroid"], "mid"),
    "simplify": (["g_simplify", "u_simplify", "u_simplify_same"], "mid"),
    # Array-array. Today's st_intersects (g_) is geoarrow-expr-geo's `relate_boolean` (DE-9IM);
    # e_ calls `geoarrow_expr_geo::intersects` (the `Intersects` trait). Each is paired with an
    # owned GeoColumn kernel using the same algorithm.
    "intersects": (
        ["g_intersects_aa", "u_intersectsrelate_aa", "e_intersects_aa", "u_intersects_aa"],
        "intersects",
    ),
    # Informative only: today's constant path is geodatafusion's own prepared code, not expr-geo.
    "intersects_const": (["g_intersects", "u_intersects"], "intersects"),
}


def h1_jobs(rows_table, styles=("t", "u", "v")):
    """`v_` is the unified style with a direct native→WKB writer (sensitivity variant)."""
    for f, cls in H1_FUNCS.items():
        for d in DATASETS:
            if f == "x" and d != "points":
                continue
            for e in ENCODINGS:
                qs = [f"{s}_{f}" for s in styles if not (s == "v" and e == "wkb")]
                if qs:
                    yield ("H1", f, d, e, rows_table[cls][d], qs)


def h2_jobs(rows_table, styles=("u", "t", "v")):
    for p, cls in H2_PIPELINES.items():
        for d in DATASETS:
            for e in ENCODINGS:
                qs = [f"h2_{p}_{s}_{o}" for s in styles for o in ("n", "w")]
                yield ("H2", p, d, e, rows_table[cls][d], qs)


def h6_jobs(rows_table):
    for f, (qs, cls) in H6_QUERIES.items():
        for d in DATASETS:
            # The array-array case materializes the 100-vertex constant per row; cap points.
            rows = rows_table[cls][d]
            if f == "intersects" and d == "points":
                rows = min(rows, rows_table["intersects"]["poly10"])
            for e in ENCODINGS:
                yield ("H6", f, d, e, rows, qs)
