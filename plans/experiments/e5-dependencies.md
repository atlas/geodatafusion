# E5: dependency cost

Decisions: D5 (`#[user_doc]`), D12 (PROJ through the `proj` crate).
Hypotheses (pre-registered in `plans/hypotheses.md`, unchanged): H5, H12.

Run on 2026-10-05. Experiment code: `experiments/e5-dependencies/` (`h5/`, `h12/`). Scratch
copies, target directories and logs: `target/experiments/e5*`.

## Summary

| Hypothesis | Result | Verdict under the rule |
|---|---|---|
| H5: `#[user_doc]` adds ≤ 5 crates and ≤ 5% to a clean build, supports PostGIS chapter labels, and gives the same `Documentation` as the builder | 0 new crates: `datafusion-macros` and `datafusion-doc` 54.0.0 are already in the graph through `datafusion` (default-features = false). Crate-only rebuild: median paired change −1.3% (debug), +2.0% (release); best-of-5 −0.4% / −0.3%. Clean full builds were too noisy to resolve 5% (paired ratios 0.65–1.08) but compile the same 226 rlibs. Custom labels work (`include: true`, `description: None`). Generated `Documentation` equals the builder's, field for field. | **Holds: migrate.** |
| H12: bundled PROJ builds on Ubuntu 24.04 in ≤ 10 min and matches PostGIS to 12 significant digits for every `ST_Transform` doc-test record | Clean release build of a `proj` + `bundled_proj` crate in `ubuntu:24.04` at 4 CPUs: 92 s, 63 s, 63 s (debug: 57 s; 2 CPUs: 93 s). 56 of 56 transformed coordinates in 25 `ST_Transform*` calls are **bitwise equal** to PostGIS's; 22 of 22 records reproduce the recorded output exactly. | **Holds: add a `proj` feature.** |

Two findings change the plan. The `proj` crate's `Proj::new(pipeline).convert` fails on the
ST_TransformPipeline example; it has to use `project` with radians or call `proj-sys`. And
bundled PROJ needs `proj.db` at runtime, which isn't embedded. See "Consequences for the plan".

## Environment

| Item | Value |
|---|---|
| Host | Arch Linux, kernel 7.2.8, AMD Ryzen 7 PRO 250 (16 threads), 60 GB RAM |
| Rust | 1.97.1 (`RUSTUP_TOOLCHAIN=1.97.1`) |
| DataFusion | 54.0.0 in `Cargo.lock` (`datafusion = { version = "54", default-features = false }`); `datafusion-macros` and `datafusion-doc` 54.0.0. 54.1.0 has an identical `user_doc.rs`. |
| `proj` / `proj-sys` | 0.31.0 / 0.27.0 (`cargo info`). `proj-sys` bundles `PROJSRC/proj-9.6.2.tar.gz` and builds it with cmake when `bundled_proj` is on, or when pkg-config finds no PROJ ≥ 9.6.2. |
| PostGIS oracle | `POSTGIS="3.6.4 94d984b"`, `PROJ="9.8.1 NETWORK_ENABLED=OFF ... DATABASE_PATH=/usr/share/proj/proj.db" (compiled against PROJ 9.6.0)`, GEOS 3.14.1. Container `geodatafusion-postgis` (`postgis/postgis:18-3.6`), 23 entries in `/usr/share/proj` (proj.db, a few `.gsb`/`.gtx` grids, no `.tif` grids). |
| Host PROJ | 9.8.1 (pkg-config), sqlite 3.53.4 |
| Container | `ubuntu:24.04` + `build-essential cmake pkg-config sqlite3 libsqlite3-dev`, rustup 1.97.1 (`experiments/e5-dependencies/h12/Containerfile`), podman 6.1.3, `--cpus 4` (GitHub's public ubuntu-24.04 runner has 4 vCPUs) |
| Load | Other agents ran heavy builds throughout. 1-minute load average 19–47 during the H5 full builds, 6–18 during the H5 crate-only builds, 6–11 during the H12 builds. Every timing row records the load before and after. |

## H5: `#[user_doc]`

### Method

1. Copied the working tree (without `target/` and `.git/`) to `target/experiments/e5/repo-before`
   and `target/experiments/e5/repo`. The real working tree wasn't touched.
2. In `repo`: added `datafusion-doc = "54"` and `datafusion-macros = "54"` to
   `[workspace.dependencies]` and to `rust/geodatafusion/Cargo.toml`. Migrated `Area`
   (`udf/geo/measurement/area.rs`, label "Measurement Functions") and `Point`
   (`udf/native/constructors/point.rs`, label "Geometry Constructors") to `#[user_doc]`, with
   `documentation()` returning `self.doc()`. Removed the `OnceLock` statics and the
   `DOC_SECTION_OTHER` imports. Full diff: `experiments/e5-dependencies/h5/migration.patch`.
3. Added a `user_doc_test` module to each file. It compares `ScalarUDF::from(..).documentation()`
   (so the path DataFusion itself uses) with `Documentation::builder(..)` holding the same
   content, using `assert_eq!` on the whole struct (`Documentation: PartialEq`). That covers
   `doc_section` (include, label, description), description, syntax, arguments, alternative
   syntax, SQL example and related UDFs. It also checks that only the section differs from the
   pre-migration builder (`DOC_SECTION_OTHER`). For `Point`, it checks the three arguments and the
   two related UDFs explicitly.
4. Dependency graph: `cargo tree -p geodatafusion -e normal,build --prefix none`, deduplicated and
   sorted, before and after (`h5/tree-before.txt`, `h5/tree-after.txt`).
5. Build time (`h5/h5_build_times.sh 3 5`), `cargo build --offline -q -p geodatafusion`, before and
   after run alternately, each in its own target directory:
   - *full*: `rm -rf` the target directory, then build (debug and release, 3 reps each);
   - *crate*: `cargo clean -p geodatafusion`, then rebuild, so the dependencies are already
     built (debug and release, 5 reps each).

### Results

**Crates.** `diff tree-before.txt tree-after.txt` is empty: 238 lines in both. The two crates
are already in the graph:

```
datafusion-doc v54.0.0       <- datafusion-expr <- datafusion
datafusion-macros v54.0.0    <- datafusion-functions <- datafusion   (proc-macro; syn, quote already present)
```

The only `Cargo.lock` change is two new entries in geodatafusion's dependency list. The
baseline release target directory already contains `libdatafusion_macros-*.so` and
`libdatafusion_doc-*.rlib`. Both target directories hold 226 rlibs.

**Tests.** `cargo test -p geodatafusion --lib user_doc` gives 2 passed (`area::user_doc_test`,
`point::user_doc_test`). The existing `area::test::test` still passes, and `cargo clippy
--all-targets` reports nothing.

**Custom section labels.** `user_doc.rs:197-205` looks the label up in `doc_sections_const()`
(DataFusion's 12 scalar sections). If it isn't there, it emits
`DocSection { include: true, label, description: None }`. The tests confirm this for "Measurement
Functions" and "Geometry Constructors". None of the 19 PostGIS chapter labels in G6 collides with
a DataFusion label, so none would silently pick up DataFusion's section. Limitations:
`doc_section(...)` only parses `label`. Other keys (`description`, `include`) are silently
ignored, so a custom section can't have a description through the macro. `include` only matters
to `Documentation::to_doc_attribute` and DataFusion's own doc generator.

**Build times (seconds; load1 before → after).**

| kind | profile | rep | before | after | after/before |
|---|---|---|---|---|---|
| full | debug | 1 | 97.2 (27.0→20.8) | 105.3 (20.8→23.6) | 1.082 |
| full | debug | 2 | 138.0 (19.2→22.1) | 131.9 (22.1→25.1) | 0.956 |
| full | debug | 3 | 224.9 (22.2→46.9) | 178.9 (46.9→32.6) | 0.796 |
| full | release | 1 | 311.2 (23.6→22.1) | 220.0 (22.1→19.2) | 0.707 |
| full | release | 2 | 313.7 (25.1→23.5) | 284.2 (23.5→22.2) | 0.906 |
| full | release | 3 | 395.0 (32.6→22.0) | 256.0 (22.0→18.6) | 0.648 |
| crate | debug | 1–5 | 6.51, 4.69, 4.51, 4.84, 6.68 | 6.69, 4.52, 4.49, 4.52, 6.60 | 1.028, 0.963, 0.996, 0.933, 0.987 |
| crate | release | 1–5 | 37.51, 32.65, 32.95, 45.91, 43.87 | 32.84, 32.56, 39.05, 48.53, 44.73 | 0.875, 0.997, 1.185, 1.057, 1.020 |

| kind | profile | median before | median after | median paired change | best-of-n change |
|---|---|---|---|---|---|
| full | debug | 138.0 | 131.9 | −4.4% | +8.2% |
| full | release | 313.7 | 256.0 | −29.3% | −29.3% |
| crate | debug | 4.84 | 4.52 | −1.3% | −0.4% |
| crate | release | 37.51 | 39.05 | +2.0% | −0.3% |

The full-build numbers are noise: the release "after" builds are 10–35% *faster* with an
identical set of crates. "Before" always ran first in each pair, so a systematic order effect is
possible. The same 226 crates compile either way, so the clean-build difference equals the
crate-only difference. Best-of-5 there is within ±0.4%, and the medians are within ±2%.

### Verdict

All conditions hold: 0 ≤ 5 crates, a clean-build change within ±2% (≤ 5%), PostGIS chapter
labels work as sections, and `Documentation` is identical. **Migrate (D5: yes).**

## H12: bundled PROJ

### Method

1. **Cases** (`h12/cases.py` → `h12/cases.tsv`). `grep -il st_transform` over `postgis_docs/`
   finds 14 files and 22 records (`h12/slt_records.py` parses them, matching
   `ST_(Inverse)Transform*`). Each `ST_Transform*` call in them is one case: 25 cases, 56
   coordinates after merging identical calls. For each case, PostGIS computes the call's input
   (after inner functions such as `ST_Centroid` or `ST_Intersection`) and its output. Both are
   dumped with `ST_DumpPoints` at full float8 precision (`extra_float_digits = 3`). The
   source/target is `EPSG:<srid>` (the oracle's `spatial_ref_sys` has `auth_srid = srid` for all
   SRIDs used) or the PROJ string the SQL passes.
2. **PROJ** (`h12/proj-check`, a standalone crate depending on `proj` 0.31 with `bundled_proj` and
   `proj-sys` 0.27). It mirrors PostGIS:
   - `proj_create_crs_to_crs` (or `proj_create` for pipelines), then
     `proj_normalize_for_visualization`;
   - for a bare conversion that can't be normalised (`EPSG::16031`), PostGIS keeps it
     unnormalised;
   - degrees↔radians when `proj_angular_input`/`proj_angular_output` say so (PostGIS
     `ptarray_transform`);
   - `proj_trans` with Z (0 for 2D input), forward, or inverse for
     `ST_InverseTransformPipeline`.

   It also runs the API geodatafusion would use, `Proj::new_known_crs(from, to, None).convert`,
   for all 54 CRS-to-CRS coordinates. Agreement uses the harness rule (`render::float`): equal
   after rounding to 12 significant digits.
3. **Records** (`h12/records.py`). Each of the 22 records is re-run in PostGIS with every
   `ST_Transform*` call replaced by a geometry literal built from PROJ's coordinates. The output
   is rendered as the harness does and compared with the expected text in the `.slt` file.
   - In `st_transform.slt #3`, `ST_Intersection` runs on PostGIS's gnomonic geometries. Their
     inputs are themselves cases (`p1_gnom`, `p2_gnom`) and were bitwise equal, so the chain is
     the same.
   - Before the real run, the script was checked by feeding it PostGIS's own coordinates: 22/22.
4. **Build** (`h12/run_container.sh [jobs] [profile]`). This runs `ubuntu:24.04` with
   `--cpus N`. `cargo fetch` runs first and isn't timed. Then the target directory is removed and
   `cargo build --profile <p>` is timed with `/usr/bin/time`. That build includes the cmake build
   of PROJ 9.6.2 (`proj-sys` disables TIFF, curl and the command-line tools). Binary size: a
   trivial `hello` binary vs `probe` (one `Proj::new_known_crs` + `convert`), both stripped.
5. Also run locally: against system PROJ 9.8.1 (`--no-default-features`, pkg-config), and with
   bundled 9.6.2.

### Results

**Build time (clean, ubuntu:24.04).**

| run | CPUs | profile | wall | user | load1 before |
|---|---|---|---|---|---|
| 1 | 4 | release | 92.1 s | 288.7 s | 11.3 |
| 2 | 4 | release | 63.1 s | 191.5 s | 8.6 |
| 3 | 4 | release | 63.7 s | 193.7 s | 8.7 |
| 4 | 4 | dev | 56.7 s | 166.0 s | 8.3 |
| 5 | 2 | release | 93.0 s | 161.7 s | 7.1 |
| local (host, 16 threads) | 16 | release | 41.7 s | 408.7 s | 5.3 |

Peak RSS was 1.69 GB. Run 1 includes a cold page cache. No step needs network access beyond
`cargo fetch`, because the PROJ source tarball ships inside `proj-sys`. The image needs
`cmake`, `sqlite3` and `libsqlite3-dev`.

**Accuracy.**

| PROJ build | coordinates agreeing to 12 sig. digits | bitwise equal | `proj` crate `convert` = raw `proj_trans` | records exact / 12 digits |
|---|---|---|---|---|
| bundled 9.6.2, ubuntu:24.04 (all 5 runs) | 56/56 | 56/56 | 54/54 | 22/22 / 22/22 |
| bundled 9.6.2, host | 56/56 | 56/56 | 54/54 | — |
| system 9.8.1, host | 56/56 | 56/56 | 54/54 | 22/22 / 22/22 |

Every record ([full table](../../experiments/e5-dependencies/h12/records-container.md)) matches
exactly: `st_3ddistance`, `st_3ddwithin`, `st_3dmaxdistance`, `st_area` ×2, `st_buffer`,
`st_distance` ×4, `st_distance_spheroid`, `st_distancesphere`, `st_inversetransformpipeline` ×2,
`st_length`, `st_point`, `st_setsrid`, `st_transform` ×3 and `st_transformpipeline` ×2. Z
values (the CIRCULARSTRING Z and the 3D distance inputs) pass through unchanged in both PROJ and
PostGIS.

PROJ 9.6.2 and 9.8.1 chose the same operation for every case; only the description wording
differs ("+ axis order change (2D)" vs "(with axis order normalized for visualization)"). None
of them uses a grid:

- NAD83→WGS84 is "NAD83 to WGS 84 (1)", a null transformation;
- 4326→2163 and 4326→3785 are ballpark offsets;
- GDA94→GDA2020 (4939→7844) is the 7-parameter Helmert "GDA94 to GDA2020 (1)". The conformal grid
  alternative isn't installed in either environment.

Network is off in both PROJ builds (`proj_context_is_network_enabled = 0`) and in PostGIS
(`NETWORK_ENABLED=OFF`).

**Binary size (stripped, release).** `hello` 368,440 B; `probe` 13,458,384 B (**+13.1 MB**,
libproj.a, sqlite bindings and libstdc++ glue); `proj-check` 13,522,360 B. In debug, `probe` is
15.2 MB stripped and libproj.a is 147.5 MB (release: 22.5 MB). On top of that, `proj.db` is
9.4 MB and isn't embedded. PROJ finds it through a compiled-in search path into the build's
`OUT_DIR` (`.../proj-sys-*/out/share/proj`), so a binary copied elsewhere needs `PROJ_DATA`. The
binary also links `libsqlite3.so.0` and `libstdc++` dynamically.

**Pipeline API.** For `ST_TransformPipeline('POINT(2 49)', 'urn:ogc:def:coordinateOperation:EPSG::16031')`:

```
Proj::new(urn).convert((2, 49))                    = Err(Conversion("Invalid coordinate"))
Proj::new(urn).project((2°, 49°) in radians, fwd)  = Ok((426857.9877165967, 5427937.523342293))  // = PostGIS
```

`proj_normalize_for_visualization` fails for this operation ("Cannot retrieve source or target
CRS"), so the code must fall back to the unnormalised operation, as PostGIS does.

### Verdict

Build ≤ 10 min: holds (57–93 s at 2–4 CPUs). 12 significant digits for every record: holds
(56/56 coordinates bitwise, 22/22 records exact). **Add a `proj` feature (D12: yes).** The
accuracy fallback in the rule ("find out why first") isn't triggered.

## Consequences for the plan

- **D5.** The "adds `datafusion-macros` and `datafusion-doc` as dependencies" cost in
  `plans/README.md` D5 and G6 R4 is nominal: they become direct dependencies but add no crates. A
  custom section can't carry a description through the macro.
- **D12 / G3 Template E.**
  - `Proj::new(pipeline)` + `convert` (G3 section 5, ST_TransformPipeline row) is wrong for
    angular operations. Use `project` with radians when `proj_angular_input` is true, or call
    `proj-sys` directly, which is needed for 3D and inverse anyway.
  - Fall back to the unnormalised operation when normalisation fails.
  - The ST_InverseTransformPipeline and Z cases above were done with ~20 lines of `proj-sys`
    (`proj_trans` with `PJ_INV` and Z). They work, which supports the "`proj-sys` directly"
    fallback in G3's crate choice.
- **CI.** The bundled build costs ~1–1.5 min at 4 CPUs, smaller than the "several minutes"
  estimated in G3. Its numbers match the 9.8.1 oracle for every doc test, so pinning the oracle's
  PROJ version isn't needed for the current records.
- **Distribution.** Plan for `proj.db` (9.4 MB) at runtime: `PROJ_DATA`, or ship it next to the
  wheel. That isn't covered in G3 today.

## Threats to validity

- **H5 timing noise.** The host was shared with other agents' builds (load 6–47 on 16 threads).
  The full-build pairs vary by ±35% and can't resolve 5%. The verdict rests on the
  identical-crate argument plus the crate-only rebuilds. Only 2 of 58 UDFs were migrated.
  Proc-macro expansion grows with the number of attributes. The crate-only debug rebuild is
  ~4.5 s in total, so even a 50× larger expansion cost would be small, but it wasn't measured.
- **H5 order effect.** "Before" always ran first in each pair.
- **H12 coverage.** The doc tests use no grid-based transformations: no NAD27, no OSTN15, and
  GDA2020 uses Helmert. Bundled PROJ has TIFF off, so it can't read `.tif` grids even if they are
  installed. PostGIS's container has only a few NTv2/GTX grids. Grid-based cases, and any record
  added later that relies on them, can differ, and this experiment doesn't test them.
- **H12 PostGIS mirroring.** proj-check reimplements PostGIS's call sequence (normalise, angular
  conversion, Z = 0 for 2D input) from its documented behaviour, not by reading its source. The
  exact match on all 56 coordinates suggests the sequence is right for these operations.
- **H12 container fidelity.** `ubuntu:24.04` with the packages listed isn't the GitHub runner
  image (which has more preinstalled, including cmake and sqlite3). `--cpus 4` limits CPU quota
  on a loaded 16-thread host, not dedicated cores. `cargo fetch` (download) isn't in the timings.
- **Inputs computed by PostGIS.** `ST_Centroid`/`ST_Intersection` inputs come from PostGIS
  (GEOS), so this tests PROJ only, not geodatafusion's geometry functions.

## Reproduction

```sh
cd /home/mikkel/Projects/geodatafusion
export RUSTUP_TOOLCHAIN=1.97.1

# H5: scratch copies (no git worktree), then apply the migration patch to `repo`.
E5=target/experiments/e5; mkdir -p $E5
rsync -a --exclude /target --exclude /.git ./ $E5/repo-before/
rsync -a $E5/repo-before/ $E5/repo/
(cd $E5 && patch -p1 -d repo < ../../../experiments/e5-dependencies/h5/migration.patch)
(cd $E5/repo-before && cargo tree -p geodatafusion -e normal,build --prefix none | sed 's/ (\*)//;s/ (proc-macro)//' | sort -u) > before.txt
(cd $E5/repo        && cargo tree -p geodatafusion -e normal,build --prefix none | sed 's/ (\*)//;s/ (proc-macro)//' | sort -u) > after.txt
diff before.txt after.txt     # empty apart from the path in the geodatafusion line
(cd $E5/repo && CARGO_TARGET_DIR=$PWD/../target-test cargo test -p geodatafusion --lib user_doc)
experiments/e5-dependencies/h5/h5_build_times.sh 3 5     # writes target/experiments/e5/h5_times.csv

# H12
cd experiments/e5-dependencies/h12
python3 cases.py                    # needs the PostGIS oracle on :54329 and psycopg2
./run_container.sh 4 release        # ubuntu:24.04, bundled PROJ, timed; writes results-container.tsv under target/experiments/e5-h12-container
python3 records.py ../../../target/experiments/e5-h12-container/results-container.tsv
# Local, system PROJ:
(cd proj-check && CARGO_TARGET_DIR=../../../../target/experiments/e5-h12-system cargo build --release --no-default-features)
../../../target/experiments/e5-h12-system/release/proj-check cases.tsv results-system.tsv
```
