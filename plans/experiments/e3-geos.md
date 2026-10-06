# E3: GEOS versions and distribution

Decisions: D7 (pin GEOS for parity), D13 (Python wheels), D4 (builds without GEOS).
Hypotheses (pre-registered in `plans/hypotheses.md`, unchanged): H7a, H7b, H13.

Run on 2026-10-05. Experiment code: `experiments/e3-geos/`. Data and build outputs:
`target/experiments/e3*`.

## Summary

| Hypothesis | Result | Verdict under the rule |
|---|---|---|
| H7a: GEOS 3.12, 3.14 and 3.15 give different results at the harness's precision | 160 of 10,895 results (1.47%) differ between 3.12 and 3.14, and 849 (7.79%) between 3.14 and 3.15. Raw GEOS 3.14.1 vs PostGIS (GEOS 3.14.1): 8 differ, all EMPTY handling in PostGIS. | **Holds: pin GEOS for parity tests.** |
| H7b: the bundled GEOS adds ≤ 3 min to a clean CI build | +49.7 s and +91.7 s wall time (4 pinned cores, heavily loaded host). The GEOS C++ build itself takes 314–322 s, but runs alongside the Rust build. | **Holds: use the static build in CI.** |
| H13: a wheel can bundle GEOS for ≤ 10 MB extra, and a wheel without GEOS loses functions | +0.30 MB (static, ST_LineMerge only), +0.83 MB (static, broad slice of GEOS), +1.78 MB (shared libs vendored by auditwheel). 4 of the 59 Python UDF classes in today's wheels move to GEOS under "Function assignments". | **Holds: bundle GEOS.** |

## H7a: GEOS version differences

### Method

- **Corpus.** `experiments/e3-geos/gen_corpus.py` (stdlib only, `random.Random(20261005)`) writes
  `target/experiments/e3/corpus.tsv` (2,007 rows, sha256 `270d6e66d46566b6…`; it regenerates
  byte for byte). The corpus is ISO WKB, with a per-row `param` of 5% of the bbox diagonal. Half
  the coordinates are full-precision doubles, the rest rounded to 6 or 3 decimals.
  - Single geometries (1,407): 17 EMPTY/degenerate cases, 300 star polygons, 150 polygons with
    holes, 100 multipolygons, 250 invalid polygons (shuffled vertices, bow-tie, spike, hole outside,
    overlapping holes, zero-area ring, overlapping parts, self-touching ring, repeated points), 200
    lines, 200 mergeable multilines (shuffled/reversed pieces of a walk, Y junctions, rings,
    disconnected parts), 100 points/multipoints, 50 collections and 40 Z geometries.
  - Pairs (600): overlapping polygons, near-coincident polygons (rotated by 1e-12…1e-4 rad or
    shifted by 1e-12…1e-5 × r), shared edges, polygon/line, line/line, holes/multipolygon and
    invalid/valid.
- **Operations** (geos crate 11.1.1, C API): `buffer(param, 8)`, `buffer(-param, 8)` (areal
  only), `make_valid`, `topology_preserve_simplify(param)`, `point_on_surface`, `convex_hull`,
  `line_merge` (lineal only) and `is_valid_reason` on singles; `intersection` and `union` on pairs.
  That gives 10,895 results per version.
- **Runner.** `experiments/e3-geos/runner` is a standalone crate. It writes the raw result
  (`G:` WKB hex with the output dimension set explicitly, `T:` text, `E:` error) and the canonical
  form. The canonical form comes from compiling the harness's own
  `rust/geodatafusion/tests/sqllogictests/render.rs` (via `#[path]`): canonical EWKT, 12
  significant digits, text verbatim, and errors as `ERROR`.
  - GEOS 3.12.1: Ubuntu 24.04 apt `libgeos-dev` in podman (`experiments/e3-geos/Containerfile`).
  - GEOS 3.14.1: `--features static` with `geos-sys` 2.0.9 / `geos-src` 0.2.4 pinned in the
    runner's `Cargo.lock`.
  - GEOS 3.15.0: the Arch system library.
- **PostGIS reference.** `run_postgis.sh` runs the same (id, op) list in one psql session (temp
  table, `pg_temp` function, exceptions caught per row) against PostGIS 3.6.4 / GEOS 3.14.1. The
  output is `ST_AsEWKB`, canonicalised by the same runner. The SQL is `ST_Buffer(g, p)`,
  `ST_MakeValid`, `ST_SimplifyPreserveTopology`, `ST_PointOnSurface`, `ST_ConvexHull`,
  `ST_LineMerge`, `ST_IsValidReason`, `ST_Intersection` and `ST_Union`.
- **Comparison.** `compare.py` compares every pair of result sets twice: raw (bit-exact) and
  canonical (the harness rule). It then puts each canonical difference into a category. "Same
  geometry up to ring start/orientation/part order" means the two results are equal after rotating
  each ring to its minimum vertex, choosing an orientation, and sorting holes and parts.
- **sqllogictests.** ST_LineMerge is the only GEOS-backed function in the crate today.
  `postgis_docs/st_linemerge` and the full suite ran under all three versions. 3.12 and 3.14.1 ran
  in the Ubuntu container, 3.15 on the host. All three used a snapshot copy of the repo
  (`make_ws.sh`).

### Raw data

Pairwise differences out of 10,895 results:

| pair | raw (bit-exact) | canonical (harness) |
|---|---|---|
| 3.12 vs 3.14 | 366 (3.36%) | 160 (1.47%) |
| 3.12 vs 3.15 | 1,380 (12.67%) | 904 (8.30%) |
| 3.12 vs PostGIS | 384 (3.52%) | 168 (1.54%) |
| 3.14 vs 3.15 | 1,266 (11.62%) | 849 (7.79%) |
| 3.14 vs PostGIS | 18 (0.17%) | 8 (0.07%) |
| 3.15 vs PostGIS | 1,282 (11.77%) | 857 (7.87%) |

Canonical differences per operation:

| op | n | 3.12 vs 3.14 | 3.12 vs 3.15 | 3.12 vs PG | 3.14 vs 3.15 | 3.14 vs PG | 3.15 vs PG |
|---|---|---|---|---|---|---|---|
| buffer | 1407 | 25 | 61 | 25 | 46 | 0 | 46 |
| buffer_neg | 825 | 0 | 0 | 0 | 0 | 0 | 0 |
| convexhull | 1407 | 0 | 0 | 6 | 0 | 6 | 6 |
| intersection | 600 | 47 | 229 | 47 | 229 | 0 | 229 |
| isvalidreason | 1407 | 3 | 3 | 3 | 0 | 0 | 0 |
| linemerge | 428 | 0 | 0 | 2 | 0 | 2 | 2 |
| makevalid | 1407 | 1 | 240 | 1 | 240 | 0 | 240 |
| pointonsurface | 1407 | 0 | 0 | 0 | 0 | 0 | 0 |
| simplifypt | 1407 | 37 | 37 | 37 | 0 | 0 | 0 |
| union | 600 | 47 | 334 | 47 | 334 | 0 | 334 |

Categories:

- **3.12 vs 3.14 (160).** These are real geometric differences. None goes away under ring
  normalization.
  - simplifypt, vertex count (37, mostly mergeable multilines). In 3.12 the topology-preserving
    simplifier keeps a ring's closing/start vertex that 3.14 removes. Example:
    `POLYGON((-38.839 -5.113,…,-38.839 -5.113))` (7 points) vs 6 points.
  - intersection and union (47 each, all near-rotated pairs). 17 have different coordinates, and
    30 differ by < 1e-9 relative but still change at 12 significant digits.
  - buffer (25: vertex count 13, coordinates 12). Collections, multilines and degenerate inputs.
  - isvalidreason (3). The reported location differs in the 12th–15th digit, e.g.
    `Self-intersection[-107.324209843923 32.1165816274309]` vs `…844001 …274125`. Reason text is
    compared verbatim, so these fail.
  - makevalid (1). 3.12 returns `GEOMETRYCOLLECTION(MULTIPOLYGON(…),…)` (with a lower-dimension
    leftover) where 3.14 returns just the `MULTIPOLYGON(…)`.
- **3.14 vs 3.15 (849).**
  - 789 are the same geometry with a different ring start vertex or orientation, or a different
    part order: union 329, intersection 229, makevalid 231.
  - The other 60 are real differences:
    - buffer 46: 14 change type (MULTIPOLYGON vs POLYGON, buffers of invalid input with
      overlapping holes or shuffled vertices), 17 change vertex count, 15 change coordinates.
    - makevalid 9: 7 change vertex count, 2 change type.
    - union 5: 3 change coordinates, 2 change vertex count.
- **GEOS 3.14.1 via the crate vs PostGIS 3.14.1 (8).** All 8 are PostGIS handling EMPTY input
  before calling GEOS. `ST_ConvexHull` of an EMPTY returns the input type's EMPTY
  (`POINT EMPTY`, `LINESTRING EMPTY`, …) where `GEOSConvexHull` returns
  `GEOMETRYCOLLECTION EMPTY` (6 cases). `ST_LineMerge` of `LINESTRING EMPTY` / `MULTILINESTRING
  EMPTY` returns the input type (2 cases). The 10 raw-only differences are the same 10 overlay
  TopologyExceptions on both sides, with different message prefixes (`GEOSUnion_r failed with` vs
  `lwgeom_union_prec: GEOS Error:`). Excluding EMPTY handling, raw GEOS 3.14.1 matched PostGIS on
  every result. That supports G3's premise (H4b), although this corpus isn't E2's.
- **Compiler check.** GEOS 3.14.1 built by `geos-src` with Ubuntu's gcc 13.3 (container) and with
  Arch's gcc 16.2 (host) gave byte-identical output files. The differences above follow the GEOS
  version, not the toolchain that built it.

sqllogictests: `postgis_docs/st_linemerge` passed 4/5 under 3.12.1, 3.14.1 and 3.15.0. The
failing record fails with `WKT error: Missing closing parenthesis` (the ZM/WKT parsing issue, not
GEOS). The full suite gave 55/582 records and 7/269 files under all three, and `parity.txt is up to
date` each time. Today's slt files don't tell the versions apart, because ST_LineMerge is the only
GEOS function and none of its records hits a changed code path.

### Verdict

The rule: "pin GEOS for parity tests if any result differs between versions." Results differ
between every pair of versions (160 between CI's 3.12 and the oracle's 3.14, 849 between 3.14 and
local 3.15). **H7a holds. Pin GEOS (3.14.1) for parity tests, as D7 proposes.**

Comments on the rule:

- Ring-start normalization in the harness would remove 789 of the 849 3.14/3.15 differences, but
  60 real differences remain. It would remove none of the 3.12/3.14 ones. Normalization doesn't
  replace pinning.
- Pinning has a trap. `geos-sys` 2.0.9 depends on `geos-src = "^0.2.4"`, so enabling `geos/static`
  against the repo's current `Cargo.lock` (`geos-sys` 2.0.9, no `geos-src` entry) resolves
  `geos-src` **0.2.5**, which is GEOS 3.15.1dev. I saw this happen. A fresh lock picks
  `geos-sys` 2.0.10, which requires 0.2.5. Use
  `cargo update -p geos-sys --precise 2.0.9 && cargo update -p geos-src --precise 0.2.4`, plus the
  Dependabot ignore G3 R6 proposes. A wrong lock gives about 8% parity differences against the
  oracle.
- The oracle's PostGIS was compiled against GEOS 3.13.1 and runs 3.14.1
  (`postgis_full_version()`). Compile-time `#if POSTGIS_GEOS_VERSION` paths in PostGIS follow
  3.13. That doesn't affect this corpus, but a future oracle image could change PostGIS's code
  paths without a GEOS runtime change.

## H7b: build cost of the bundled GEOS

### Method

`experiments/e3-geos/h7b.sh` runs inside the `e3-ubuntu` image (Ubuntu 24.04, gcc 13.3, cmake
3.28, apt GEOS 3.12.1), pinned to 4 cores with `taskset -c 0-3` (`nproc` = 4). Each build uses a
fresh `CARGO_TARGET_DIR` and runs
`cargo test --workspace --no-run --offline --features <F> --timings`, CI's `cargo test
--all-features` compile step, on the snapshot copy with the plan's
`geos-static = ["geos-3_11", "geos/static"]` feature and `geos-src` pinned to 0.2.4.

- `system`: `F = geodatafusion/geos-3_11` (links apt GEOS).
- `static`: `F = geodatafusion/geos-static` (builds GEOS 3.14.1 with cmake, Release).

Two rounds ran in opposite orders. The toolchain is Rust 1.97.1 (host rustup mounted read-only),
and the crate sources came from the host registry (no download time). Per-unit times come from
`cargo --timings`.

### Raw data

Host: AMD Ryzen 7 PRO 250 (8 cores / 16 threads), 60 GB RAM. Other agents were running heavy
builds the whole time, so the host load average was 22–29 on 16 threads, and cores 0–3 were
shared with them.

| round | variant | wall (s) | load avg before | load avg after |
|---|---|---|---|---|
| c4a | system | 438.0 | 26.13 22.08 13.63 | 25.69 25.58 18.75 |
| c4a | static | 487.7 | 25.69 25.58 18.75 | 28.71 28.25 23.43 |
| c4b | static | 494.6 | 22.35 26.35 23.07 | 29.41 27.52 24.98 |
| c4b | system | 402.9 | 29.41 27.52 24.98 | 25.42 27.11 25.78 |

- Added wall time: +49.7 s (c4a) and +91.7 s (c4b), mean +70.7 s.
- `geos-src` build-script run (the GEOS C++ build): 313.9 s (c4a, from 120.3 s to 434.2 s) and
  321.7 s (c4b, from 120.0 s to 441.7 s). In both rounds it ran alongside the DataFusion crates
  and finished 5–8 s after `datafusion` itself (436.9 s in c4b). It was nearly on the critical
  path.
- Other data points, same loaded host:
  - The runner crate with `--features static`, release, 16 threads: 142.9 s clean (load 12→30).
    The same build in the container: 3 min 07 s.
  - A shared GEOS 3.14.1 cmake build in the manylinux_2_28 image: 186 s on 16 threads.

### Verdict

The rule: "if it holds [≤ 3 minutes added to a clean CI build], use the static build in CI."
Measured: +50 s and +92 s. **H7b holds. Use the static build in CI.**

Comments:

- The low added cost depends on overlap. GEOS alone needs about 5.3 minutes on 4 contended cores
  and finished just after the DataFusion crates. If the Rust side gets faster relative to GEOS,
  or a workflow compiles little Rust, GEOS becomes the critical path. The wall-time cost then
  approaches the GEOS build time (about 3–5 minutes on 4 vCPUs). Examples: a job with a warm Rust
  cache but a cold GEOS build, or the `docs` job.
- In the CI layout, clippy, check and test in one job share the build-script output (same dev
  profile), so GEOS builds once per job. `docs-no-warnings` runs on a separate runner and builds
  it again. `Swatinem/rust-cache` caches the result, so the cost applies only on cache misses.
- The heavy, uneven host load makes the absolute times noisy. The two rounds differ by 42 s in
  the added cost.

## H13: Python wheels with and without GEOS

### What today's wheels contain

`python/Cargo.toml` depends on `geodatafusion` with no features, and `[tool.maturin] features =
["pyo3/extension-module"]`. The wheel workflow runs `maturin-action` with no `--features`. The
published wheels therefore contain no GEOS. I downloaded the PyPI
`geodatafusion-0.3.1-cp39-abi3-manylinux_2_17_x86_64` wheel (8,937,922 bytes, `_rust.abi3.so`
31.9 MB): it has 59 UDF classes, matching `python/src` registrations, and no GEOS strings.

- `geodatafusion.native` (34): CoordDim, EndPoint, IsClosed, M, GeometryType, NDims, NPoints,
  NumInteriorRings, StartPoint, STGeometryType, X, Y, Z, Box2D, Box3D, Extent, MakeBox2D,
  MakeBox3D, XMax, XMin, YMax, YMin, ZMax, ZMin, Point, PointZ, PointM, PointZM, MakePoint,
  MakePointM, AsBinary, AsText, GeomFromText, GeomFromWKB.
- `geodatafusion.geo` (22): Area, Distance, Length, Centroid, ConvexHull, OrientedEnvelope,
  PointOnSurface, Simplify, SimplifyPreserveTopology, SimplifyVW, Contains, CoveredBy, Covers,
  Crosses, Disjoint, Equals, Intersects, Overlaps, Touches, Within, IsValid, IsValidReason.
- `geodatafusion.geohash` (3): GeoHash, Box2DFromGeoHash, PointFromGeoHash.

Functions that would disappear from a wheel without GEOS if moved per `plans/README.md` "Function
assignments" (G3 rows):

- **ST_IsValid and ST_IsValidReason** (validity family to GEOS).
- **ST_SimplifyPreserveTopology** (G3).
- **ST_PointOnSurface** (G3). D4 allows a JTS-style native implementation instead, which would
  keep it.

The other G3 rows (MakeValid, IsValidDetail, ConcaveHull, DFullyWithin, IsSimple, IsRing, WrapX,
AsMVTGeom, BdPolyFromText, BdMPolyFromText) aren't in the wheels today. E2 (H4a) may move more of
the contested `geo` functions in the wheel to GEOS: ConvexHull, OrientedEnvelope, Simplify,
SimplifyVW, Centroid, Area, Length, Distance, Contains, Intersects, Within and Touches.

### Method

`experiments/e3-geos/h13.sh` runs in `quay.io/pypa/manylinux_2_28_x86_64` (gcc 14.2,
auditwheel 6.8.2). It builds wheels with maturin (run through `uv tool run 'maturin>=1.7,<2'`),
`--release -i python3.10 --auditwheel skip`, then runs `auditwheel repair --plat
manylinux_2_28_x86_64` on each. The wheels come from the snapshot copy of `python/` with
`ws.patch` applied, which adds:

- feature `geos` = `geodatafusion/geos-3_11`, plus a `geodatafusion.geos` module with
  `LineMerge`;
- feature `geos-static` = `geos` + `geodatafusion/geos-static`;
- feature `geos-wide`, a probe function that calls about 45 GEOS operations. It approximates how
  much of GEOS the G3 function set will link: overlay ops, buffer with params, make-valid,
  simplify, hulls, Voronoi, Delaunay, offset curve, node, build area, snap, shared paths,
  polygonize, MIC, clearance, densify, set precision, prepared predicates, relate, distances,
  validity and is-simple.

Without such references, static linking drops the unused GEOS code, so a LineMerge-only build
understates the cost.

The shared variants link against GEOS 3.14.1, built as shared libraries from the same source
`geos-src` 0.2.4 bundles. auditwheel copies `libgeos` and `libgeos_c` into `geodatafusion.libs/`
and sets the RPATH, as Shapely does. Every repaired wheel was smoke-tested in a clean
manylinux container: import, `LineMerge()`, and `wide()` buffer and Voronoi calls.

### Raw data

| wheel | bytes | Δ vs no GEOS | `_rust.abi3.so` (uncompressed) | vendored GEOS libs (uncompressed / compressed) |
|---|---|---|---|---|
| w0 no GEOS | 8,261,082 | — | 35,488,920 | — |
| w1 static, LineMerge | 8,561,511 | +300,429 (+0.30 MB) | 36,342,264 | — |
| w2 static, wide probe | 9,089,484 | +828,402 (+0.83 MB) | 37,832,464 | — |
| w3 shared + auditwheel, LineMerge | 10,043,270 | +1,782,188 (+1.78 MB) | 35,545,937 | libgeos 5,254,185 / 1,607,551; libgeos_c 649,281 / 161,178 |
| w4 shared + auditwheel, wide probe | 10,060,710 | +1,799,628 (+1.80 MB) | 35,616,777 | same |

Reference: Shapely 2.1.2's `cp312 manylinux2014_x86_64` wheel vendors `libgeos.so.3.13.1`
(5,315,353 bytes / 1,591,999 compressed) and `libgeos_c.so.1.19.2` (514,225 / 137,580), and ships
`LICENSE_GEOS`.

Build times (manylinux container, 16 threads, loaded host): w0 404 s clean; w1 +181 s, mostly the
GEOS build; w3 86 s; w2 and w4 7 s each (incremental).

### Verdict

The rule: "bundle GEOS if the wheel stays within the budget [≤ 10 MB extra on Linux]." Measured:
+0.3 to +1.8 MB, and the full GEOS shared libraries add 1.77 MB compressed (5.9 MB installed).
Losing functions without GEOS is also confirmed: 4 of the 59 classes. **H13 holds. Bundle GEOS.**
The fallback clause ("keep a `geo` implementation until it is") doesn't apply, because bundling is
possible within budget.

### Licensing facts (not legal advice)

- GEOS is licensed LGPL-2.1: `geos-src-0.2.4/source/COPYING` is the GNU LGPL version 2.1 text.
  The `geos`, `geos-sys` and `geos-src` crates declare `license = "MIT"` in their `Cargo.toml`,
  but `geos-src` contains and builds that LGPL source. Cargo's license metadata alone won't flag
  the LGPL code.
- LGPL-2.1 §6 (<https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html>) lets a "work that uses
  the Library" combined with it be distributed "under terms of your choice", provided:
  - the terms permit modification for the customer's own use and reverse engineering for
    debugging;
  - each copy carries prominent notice and a copy of the license;
  - one of §6a–e holds. §6a: ship the library's source and, for a linked executable, the work as
    object code or source "so that the user can modify the Library and then relink". §6b: "use a
    suitable shared library mechanism", one that "uses at run time a copy of the library already
    present on the user's computer system, rather than copying library functions into the
    executable" and works with a modified, interface-compatible version.
- Static linking (w1/w2) copies GEOS code into `_rust.abi3.so`, so §6b's mechanism doesn't apply.
  Another of §6's options would be needed, e.g. §6a's relinkable materials.
- Shared bundling (w3/w4) keeps GEOS as separate `.so` files in `geodatafusion.libs/`, which a
  user can replace with an interface-compatible build. This is Shapely's distribution model
  (vendored `libgeos*.so` and `LICENSE_GEOS` in the wheel).
- The plan's D13 ("bundle as a shared library") matches w3/w4. Shared is also the smallest change
  to the wheel workflow: build GEOS in the manylinux image before maturin, then let maturin's
  auditwheel or `auditwheel repair` vendor it. The static variant is smaller (+0.8 MB vs +1.8 MB)
  but has the §6 implications above.

## Threats to validity

- **Corpus.** It's synthetic. It has no real-world data (the NYC fixtures aren't used), and its
  Z coverage is small (40 rows). It deliberately includes near-coincident and invalid geometries,
  where version differences concentrate, so the percentages overstate what a typical doc-test
  slt sees. The rule depends on any difference existing, not on the rate.
- **Operations.** It uses default parameters only: buffer with quad_segs 8, no join or cap
  styles, make-valid with the default method, no `gridSize`. Other GEOS functions (Voronoi, offset
  curve, concave hull) aren't covered and are known to change between versions.
- **PostGIS wrappers.** The comparison calls raw C API functions, while PostGIS wraps them
  (EMPTY short-circuits, `lwgeom_make_geos_friendly`, error prefixes). Only EMPTY handling showed
  up here. The oracle's PostGIS was compiled against GEOS 3.13.1 but runs 3.14.1.
- **GEOS 3.12 build.** 3.12.1 is Ubuntu's distro build, so its compiler and flags differ from
  geos-src's. The gcc 13 vs gcc 16 check covers 3.14.1 only.
- **Build times.**
  - The host was heavily loaded by other agents (load 22–29 on 16 threads), and `taskset`
    pinning shares cores with them. A GitHub runner has 4 dedicated vCPUs with different
    per-core speed.
  - Crate downloads were excluded (offline, host registry).
  - The added time is a difference of two noisy numbers. I took two rounds, not a distribution.
- **Wheels.**
  - Only Linux x86_64, manylinux_2_28 (CI's x86_64 uses manylinux2014; aarch64 uses 2_28). macOS,
    Windows, i686, armv7 and ppc64le weren't built. ppc64le's published wheel is already 26.7 MB.
  - The wide probe approximates, but isn't, the eventual G3 function set.
  - Wheels aren't stripped (neither are today's), so absolute sizes could shrink with
    `strip = true`.
- **Snapshot.** The repo working tree was snapshotted while other agents were editing it. All
  version comparisons used the same snapshot.

## Reproduction

Run from the repo root. Prefix cargo with `RUSTUP_TOOLCHAIN=1.97.1`.

```bash
# Images
podman build -t e3-ubuntu -f experiments/e3-geos/Containerfile experiments/e3-geos
podman pull quay.io/pypa/manylinux_2_28_x86_64

# H7a corpus and runs
python3 experiments/e3-geos/gen_corpus.py target/experiments/e3/corpus.tsv
(cd experiments/e3-geos/runner && CARGO_TARGET_DIR=$PWD/../../../target/experiments/e3-runner-sys cargo build --release)
(cd experiments/e3-geos/runner && CARGO_TARGET_DIR=$PWD/../../../target/experiments/e3-runner-static cargo build --release --features static)
target/experiments/e3-runner-sys/release/e3-geos-runner run target/experiments/e3/corpus.tsv target/experiments/e3/geos-3.15.tsv
target/experiments/e3-runner-static/release/e3-geos-runner run target/experiments/e3/corpus.tsv target/experiments/e3/geos-3.14.tsv
experiments/e3-geos/in-ubuntu.sh bash -c 'cd experiments/e3-geos/runner && CARGO_TARGET_DIR=/work/target/experiments/e3-runner-u2404 cargo build --release --offline && /work/target/experiments/e3-runner-u2404/release/e3-geos-runner run /work/target/experiments/e3/corpus.tsv /work/target/experiments/e3/geos-3.12.tsv'
experiments/e3-geos/run_postgis.sh target/experiments/e3/corpus.tsv target/experiments/e3/geos-3.15.tsv target/experiments/e3/postgis-3.14.tsv target/experiments/e3-runner-sys/release/e3-geos-runner
python3 experiments/e3-geos/compare.py target/experiments/e3   # tables; diffs-*.tsv per pair

# Snapshot copy with geos-static feature and Python geos module (ws.patch), pinned to GEOS 3.14.1
experiments/e3-geos/make_ws.sh

# H7b (4 cores, Ubuntu 24.04)
experiments/e3-geos/in-ubuntu.sh taskset -c 0-3 experiments/e3-geos/h7b.sh c4a system static
cat target/experiments/e3/h7b/results.tsv

# slt per version (reuses the H7b target dirs; host run for 3.15)
experiments/e3-geos/in-ubuntu.sh bash -c 'cd target/experiments/e3/ws && CARGO_TARGET_DIR=/work/target/experiments/e3-h7b-c4a-system cargo test --workspace --offline --features geodatafusion/geos-3_11 --test sqllogictests -- -v st_linemerge'
experiments/e3-geos/in-ubuntu.sh bash -c 'cd target/experiments/e3/ws && CARGO_TARGET_DIR=/work/target/experiments/e3-h7b-c4a-static cargo test --workspace --offline --features geodatafusion/geos-static --test sqllogictests -- -v st_linemerge'
(cd target/experiments/e3/ws && CARGO_TARGET_DIR=$PWD/../../e3-slt-host cargo test --offline -p geodatafusion --features geos-3_11 --test sqllogictests -- -v st_linemerge)

# H13 wheels
podman run --rm -v "$PWD":/work -v ~/.rustup:/root/.rustup:ro -v ~/.cargo/registry:/root/.cargo/registry \
  -w /work quay.io/pypa/manylinux_2_28_x86_64 experiments/e3-geos/h13.sh
cat target/experiments/e3/h13/wheel-sizes.txt
```
