#!/usr/bin/env python3
"""Generate the fixed, seeded E3 corpus (stdlib only).

Output: a TSV with columns `id kind param a_wkb_hex b_wkb_hex`.
- `kind` names the generator, `param` is a float used by buffer/simplify (derived from the
  geometry's size), `b_wkb_hex` is set only for overlay pairs.
- All geometries are ISO WKB, little endian; Z only for the `z_*` kinds.

Usage: python3 gen_corpus.py [out.tsv]   (default target/experiments/e3/corpus.tsv)
"""

import math
import random
import struct
import sys

SEED = 20261005
rng = random.Random(SEED)

# ---------------------------------------------------------------- WKB encoding

def _hdr(code, z):
    return struct.pack("<BI", 1, code + (1000 if z else 0))


def _coords(cs, z):
    out = b""
    for c in cs:
        out += struct.pack("<ddd", *c) if z else struct.pack("<dd", c[0], c[1])
    return out


def wkb(g):
    """g = (type, z, payload)."""
    t, z, p = g
    if t == "Point":
        if p is None:
            return _hdr(1, z) + _coords([(math.nan,) * 3], z)
        return _hdr(1, z) + _coords([p], z)
    if t == "LineString":
        return _hdr(2, z) + struct.pack("<I", len(p)) + _coords(p, z)
    if t == "Polygon":
        out = _hdr(3, z) + struct.pack("<I", len(p))
        for ring in p:
            out += struct.pack("<I", len(ring)) + _coords(ring, z)
        return out
    codes = {"MultiPoint": 4, "MultiLineString": 5, "MultiPolygon": 6, "GeometryCollection": 7}
    out = _hdr(codes[t], z) + struct.pack("<I", len(p))
    for child in p:
        out += wkb(child)
    return out


def P(c, z=False):
    return ("Point", z, c)


def L(cs, z=False):
    return ("LineString", z, cs)


def Poly(rings, z=False):
    return ("Polygon", z, rings)


def Multi(t, parts, z=False):
    return (t, z, parts)


# ---------------------------------------------------------------- helpers

def coord(x, y, rnd):
    if rnd is not None:
        return (round(x, rnd), round(y, rnd))
    return (x, y)


def pick_round():
    # Half full-precision doubles, half rounded to a typical decimal precision.
    return rng.choice([None, None, 6, 3])


def star(cx, cy, r, n, rnd, ccw=True, irregular=0.7):
    angles = sorted(rng.uniform(0, 2 * math.pi) for _ in range(n))
    pts = []
    for a in angles:
        rr = r * rng.uniform(1 - irregular, 1.0)
        pts.append(coord(cx + rr * math.cos(a), cy + rr * math.sin(a), rnd))
    if not ccw:
        pts.reverse()
    # Drop accidental duplicates from rounding, then close.
    dedup = [pts[0]]
    for p in pts[1:]:
        if p != dedup[-1]:
            dedup.append(p)
    if len(dedup) < 3:
        return star(cx, cy, r, n, None, ccw, irregular)
    return dedup + [dedup[0]]


def rand_center():
    return rng.uniform(-180, 180), rng.uniform(-80, 80)


def rand_radius():
    return 10 ** rng.uniform(-3, 1)


def walk(n, cx, cy, step, rnd):
    x, y = cx, cy
    pts = [coord(x, y, rnd)]
    heading = rng.uniform(0, 2 * math.pi)
    for _ in range(n - 1):
        heading += rng.gauss(0, 0.8)
        x += step * math.cos(heading) * rng.uniform(0.2, 1)
        y += step * math.sin(heading) * rng.uniform(0.2, 1)
        c = coord(x, y, rnd)
        if c != pts[-1]:
            pts.append(c)
    if len(pts) < 2:
        pts.append(coord(x + step, y, rnd))
    return pts


def bbox_diag(cs):
    xs = [c[0] for c in cs]
    ys = [c[1] for c in cs]
    return math.hypot(max(xs) - min(xs), max(ys) - min(ys)) or 1.0


def all_coords(g):
    t, z, p = g
    if t == "Point":
        return [] if p is None else [p]
    if t == "LineString":
        return list(p)
    if t == "Polygon":
        return [c for ring in p for c in ring]
    return [c for child in p for c in all_coords(child)]


def rotate(cs, cx, cy, ang):
    ca, sa = math.cos(ang), math.sin(ang)
    return [(cx + (x - cx) * ca - (y - cy) * sa, cy + (x - cx) * sa + (y - cy) * ca) for x, y in cs]


# ---------------------------------------------------------------- generators (single)

def g_star():
    cx, cy = rand_center()
    return Poly([star(cx, cy, rand_radius(), rng.randint(5, 60), pick_round())])


def g_holes():
    cx, cy = rand_center()
    r = rand_radius()
    rnd = pick_round()
    shell = star(cx, cy, r, rng.randint(8, 60), rnd, irregular=0.2)
    holes = []
    k = rng.randint(1, 3)
    for i in range(k):
        a = 2 * math.pi * i / k + rng.uniform(0, 0.5)
        hx, hy = cx + 0.45 * r * math.cos(a), cy + 0.45 * r * math.sin(a)
        holes.append(star(hx, hy, 0.2 * r, rng.randint(4, 12), rnd, ccw=False, irregular=0.3))
    return Poly([shell] + holes)


def g_multipolygon():
    cx, cy = rand_center()
    r = rand_radius()
    rnd = pick_round()
    parts = []
    for i in range(rng.randint(2, 4)):
        parts.append(Poly([star(cx + 2.5 * r * i, cy + rng.uniform(-r, r), r, rng.randint(4, 30), rnd)]))
    return Multi("MultiPolygon", parts)


def g_invalid():
    cx, cy = rand_center()
    r = rand_radius()
    rnd = pick_round()
    variant = rng.randrange(9)
    if variant == 0:  # shuffled vertices: self-intersecting shell
        ring = star(cx, cy, r, rng.randint(5, 25), rnd)[:-1]
        rng.shuffle(ring)
        return "inv_shuffled", Poly([ring + [ring[0]]])
    if variant == 1:  # bow-tie
        s = r
        ring = [coord(cx, cy, rnd), coord(cx + s, cy + s, rnd), coord(cx + s, cy, rnd),
                coord(cx, cy + s, rnd), coord(cx, cy, rnd)]
        return "inv_bowtie", Poly([ring])
    if variant == 2:  # spike: out and back along the same line
        ring = star(cx, cy, r, rng.randint(6, 20), rnd)[:-1]
        i = rng.randrange(len(ring))
        x, y = ring[i]
        spike = coord(x + (x - cx) * 2, y + (y - cy) * 2, rnd)
        ring = ring[: i + 1] + [spike, ring[i]] + ring[i + 1:]
        return "inv_spike", Poly([ring + [ring[0]]])
    if variant == 3:  # hole outside the shell
        shell = star(cx, cy, r, 10, rnd, irregular=0.1)
        hole = star(cx + 3 * r, cy, 0.3 * r, 6, rnd, ccw=False, irregular=0.1)
        return "inv_hole_outside", Poly([shell, hole])
    if variant == 4:  # overlapping holes
        shell = star(cx, cy, r, 16, rnd, irregular=0.05)
        h1 = star(cx, cy, 0.4 * r, 8, rnd, ccw=False, irregular=0.1)
        h2 = star(cx + 0.2 * r, cy, 0.4 * r, 8, rnd, ccw=False, irregular=0.1)
        return "inv_overlapping_holes", Poly([shell, h1, h2])
    if variant == 5:  # zero-area (collinear) ring
        dx, dy = rng.uniform(-r, r), rng.uniform(-r, r)
        ring = [coord(cx + t * dx, cy + t * dy, rnd) for t in (0, 0.3, 1, 0.6)]
        return "inv_zero_area", Poly([ring + [ring[0]]])
    if variant == 6:  # overlapping multipolygon parts
        a = Poly([star(cx, cy, r, 12, rnd)])
        b = Poly([star(cx + 0.5 * r, cy, r, 12, rnd)])
        return "inv_overlapping_parts", Multi("MultiPolygon", [a, b])
    if variant == 7:  # self-touching ring (inverted shell)
        s = r
        ring = [coord(cx, cy, rnd), coord(cx + 2 * s, cy, rnd), coord(cx + 2 * s, cy + 2 * s, rnd),
                coord(cx + s, cy, rnd), coord(cx, cy + 2 * s, rnd), coord(cx, cy, rnd)]
        return "inv_self_touch", Poly([ring])
    # repeated points: valid, but stresses simplification and buffer
    ring = star(cx, cy, r, rng.randint(6, 20), rnd)[:-1]
    out = []
    for p in ring:
        out.extend([p] * rng.randint(1, 3))
    return "dup_points", Poly([out + [out[0]]])


def g_line():
    cx, cy = rand_center()
    return L(walk(rng.randint(2, 50), cx, cy, rand_radius(), pick_round()))


def g_mergeable():
    """MultiLineString from pieces of a walk, shuffled/reversed, sometimes with a branch or ring."""
    cx, cy = rand_center()
    step = rand_radius()
    rnd = pick_round()
    pts = walk(rng.randint(6, 40), cx, cy, step, rnd)
    pieces = []
    i = 0
    while i < len(pts) - 1:
        j = min(len(pts) - 1, i + rng.randint(1, 6))
        pieces.append(pts[i: j + 1])
        i = j
    if rng.random() < 0.3:  # Y junction
        k = rng.randrange(len(pts))
        pieces.append([pts[k]] + walk(rng.randint(2, 5), pts[k][0], pts[k][1], step, rnd)[1:] or [pts[k], coord(pts[k][0] + step, pts[k][1], rnd)])
    if rng.random() < 0.2:  # closed ring piece
        pieces.append(star(cx, cy - 5 * step, step, 5, rnd))
    if rng.random() < 0.2:  # disconnected piece
        pieces.append(walk(3, cx + 50 * step, cy, step, rnd))
    pieces = [p if rng.random() < 0.5 else list(reversed(p)) for p in pieces if len(p) >= 2]
    rng.shuffle(pieces)
    return Multi("MultiLineString", [L(p) for p in pieces])


def g_points():
    cx, cy = rand_center()
    r = rand_radius()
    rnd = pick_round()
    if rng.random() < 0.3:
        return P(coord(cx, cy, rnd))
    n = rng.randint(2, 40)
    return Multi("MultiPoint", [P(coord(cx + rng.uniform(-r, r), cy + rng.uniform(-r, r), rnd)) for _ in range(n)])


def g_collection():
    parts = [g_star(), g_line(), g_points()]
    rng.shuffle(parts)
    return Multi("GeometryCollection", parts[: rng.randint(1, 3)])


def g_z():
    cx, cy = rand_center()
    r = rand_radius()
    if rng.random() < 0.5:
        cs = [(x, y, rng.uniform(0, 100)) for x, y in star(cx, cy, r, rng.randint(5, 20), None)[:-1]]
        return "z_polygon", Poly([cs + [cs[0]]], z=True)
    cs = [(x, y, rng.uniform(0, 100)) for x, y in walk(rng.randint(2, 20), cx, cy, r, None)]
    return "z_line", L(cs, z=True)


def degenerate():
    out = [
        ("empty_point", P(None)),
        ("empty_line", L([])),
        ("empty_polygon", Poly([])),
        ("empty_multipoint", Multi("MultiPoint", [])),
        ("empty_multiline", Multi("MultiLineString", [])),
        ("empty_multipolygon", Multi("MultiPolygon", [])),
        ("empty_collection", Multi("GeometryCollection", [])),
        ("degen_line_same_points", L([(1.0, 1.0), (1.0, 1.0)])),
        ("degen_line_collinear", L([(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (0.5, 0.5)])),
        ("degen_polygon_collinear", Poly([[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (0.0, 0.0)]])),
        ("degen_tiny_polygon", Poly([[(0.0, 0.0), (1e-12, 0.0), (1e-12, 1e-12), (0.0, 0.0)]])),
        ("degen_huge_coords", Poly([[(1e15, 1e15), (1e15 + 1, 1e15), (1e15 + 1, 1e15 + 1), (1e15, 1e15)]])),
        ("degen_unit_square", Poly([[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]])),
        ("degen_cw_square", Poly([[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]])),
        ("degen_multiline_one_point_gap", Multi("MultiLineString", [L([(0.0, 0.0), (1.0, 0.0)]), L([(1.0, 0.0), (2.0, 0.0)]), L([(2.0, 0.0), (2.0, 1.0)])])),
        ("degen_multiline_cross", Multi("MultiLineString", [L([(0.0, 0.0), (2.0, 2.0)]), L([(0.0, 2.0), (2.0, 0.0)])])),
        ("degen_gc_nested", Multi("GeometryCollection", [Multi("GeometryCollection", [P((1.0, 2.0))]), L([(0.0, 0.0), (1.0, 1.0)])])),
    ]
    return out


# ---------------------------------------------------------------- pairs

def pair():
    cx, cy = rand_center()
    r = rand_radius()
    rnd = pick_round()
    v = rng.randrange(8)
    if v == 0:
        a = Poly([star(cx, cy, r, rng.randint(5, 40), rnd)])
        b = Poly([star(cx + rng.uniform(-r, r), cy + rng.uniform(-r, r), r, rng.randint(5, 40), rnd)])
        return "pair_overlap", a, b
    if v == 1:  # near-coincident: rotated by a tiny angle
        ring = star(cx, cy, r, rng.randint(5, 40), None)
        ang = 10 ** rng.uniform(-12, -4)
        return "pair_near_rotated", Poly([ring]), Poly([rotate(ring, cx, cy, ang)])
    if v == 2:  # near-coincident: translated by a tiny offset
        ring = star(cx, cy, r, rng.randint(5, 40), None)
        e = r * 10 ** rng.uniform(-12, -5)
        return "pair_near_shifted", Poly([ring]), Poly([[(x + e, y - e) for x, y in ring]])
    if v == 3:
        a = Poly([star(cx, cy, r, rng.randint(5, 40), rnd)])
        b = L(walk(rng.randint(2, 20), cx - r, cy, r / 3, rnd))
        return "pair_poly_line", a, b
    if v == 4:
        a = L(walk(rng.randint(2, 30), cx, cy, r, rnd))
        b = L(walk(rng.randint(2, 30), cx + r, cy, r, rnd))
        return "pair_line_line", a, b
    if v == 5:
        return "pair_holes_multi", g_holes(), g_multipolygon()
    if v == 6:  # shared edge: two halves of a polygon split along a straight line
        x0, x1, y0, y1 = cx - r, cx + r, cy - r, cy + r
        xm = coord(cx + rng.uniform(-r, r) / 2, 0, rnd)[0]
        a = Poly([[coord(x0, y0, rnd), (xm, coord(0, y0, rnd)[1]), (xm, coord(0, y1, rnd)[1]), coord(x0, y1, rnd), coord(x0, y0, rnd)]])
        b = Poly([[(xm, coord(0, y0, rnd)[1]), coord(x1, y0, rnd), coord(x1, y1, rnd), (xm, coord(0, y1, rnd)[1]), (xm, coord(0, y0, rnd)[1])]])
        return "pair_shared_edge", a, b
    kind, a = g_invalid()
    return "pair_invalid_" + kind, a, Poly([star(cx, cy, r, 10, rnd)])


# ---------------------------------------------------------------- main

def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "target/experiments/e3/corpus.tsv"
    rows = []

    def add(kind, a, b=None):
        diag = bbox_diag(all_coords(a)) if all_coords(a) else 1.0
        param = float(f"{diag * 0.05:.6g}")
        rows.append((kind, param, wkb(a).hex(), wkb(b).hex() if b is not None else ""))

    for kind, g in degenerate():
        add(kind, g)
    for _ in range(300):
        add("star", g_star())
    for _ in range(150):
        add("holes", g_holes())
    for _ in range(100):
        add("multipolygon", g_multipolygon())
    for _ in range(250):
        add(*g_invalid())
    for _ in range(200):
        add("line", g_line())
    for _ in range(200):
        add("mergeable", g_mergeable())
    for _ in range(100):
        add("points", g_points())
    for _ in range(50):
        add("collection", g_collection())
    for _ in range(40):
        add(*g_z())
    for _ in range(600):
        kind, a, b = pair()
        add(kind, a, b)

    with open(out_path, "w") as f:
        f.write("id\tkind\tparam\ta\tb\n")
        for i, (kind, param, a, b) in enumerate(rows):
            f.write(f"{i}\t{kind}\t{param!r}\t{a}\t{b}\n")
    print(f"wrote {len(rows)} rows to {out_path}")


if __name__ == "__main__":
    main()
