#!/usr/bin/env bash
# H13: build geodatafusion Python wheels with and without GEOS, in the manylinux_2_28 image.
# Host usage:
#   podman run --rm -v "$PWD":/work -v ~/.rustup:/root/.rustup:ro -v ~/.cargo/registry:/root/.cargo/registry \
#     -w /work quay.io/pypa/manylinux_2_28_x86_64 experiments/e3-geos/h13.sh
# Uses the E3 copy of python/ in target/experiments/e3/ws/python (features geos, geos-static, geos-wide).
set -euo pipefail
export RUSTUP_TOOLCHAIN=1.97.1
export PATH=/root/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH
E3=/work/target/experiments/e3
OUT=$E3/h13
WS=$E3/ws
export CARGO_TARGET_DIR=/work/target/experiments/e3-h13-target
mkdir -p "$OUT/wheels"

# Shared GEOS 3.14.1 from the same source geos-src 0.2.4 bundles.
PREFIX=$OUT/geos-3.14.1-shared
if [ ! -f "$PREFIX/lib64/libgeos_c.so" ] && [ ! -f "$PREFIX/lib/libgeos_c.so" ]; then
  SRC=$(ls -d /root/.cargo/registry/src/*/geos-src-0.2.4/source | head -1)
  rm -rf "$OUT/geos-build" && mkdir -p "$OUT/geos-build"
  s=$(date +%s)
  cmake -S "$SRC" -B "$OUT/geos-build" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON \
    -DBUILD_TESTING=OFF -DBUILD_BENCHMARKS=OFF -DBUILD_DOCUMENTATION=OFF -DCMAKE_INSTALL_PREFIX="$PREFIX" > "$OUT/geos-cmake.log"
  cmake --build "$OUT/geos-build" -j"$(nproc)" > "$OUT/geos-build.log"
  cmake --install "$OUT/geos-build" > /dev/null
  echo "shared GEOS build: $(( $(date +%s) - s )) s on $(nproc) cpus" | tee "$OUT/geos-shared-build-time.txt"
fi
LIBDIR=$(dirname "$(ls "$PREFIX"/lib*/libgeos_c.so | head -1)")

/opt/python/cp312-cp312/bin/python -m pip install -q uv
UV=/opt/python/cp312-cp312/bin/uv

build() {  # name, features, extra env...
  local name=$1 feats=$2; shift 2
  rm -rf "$OUT/wheels/$name" && mkdir -p "$OUT/wheels/$name"
  local s=$(date +%s)
  (cd "$WS/python" && env "$@" "$UV" tool run --python /opt/python/cp312-cp312/bin/python 'maturin>=1.7,<2' build \
     --release -i /opt/python/cp310-cp310/bin/python --auditwheel skip \
     ${feats:+--features $feats} -o "$OUT/wheels/$name/raw") > "$OUT/wheels/$name.log" 2>&1
  echo "$name build: $(( $(date +%s) - s )) s" >> "$OUT/build-times.txt"
  LD_LIBRARY_PATH="$LIBDIR" auditwheel repair --plat manylinux_2_28_x86_64 -w "$OUT/wheels/$name/repaired" "$OUT"/wheels/$name/raw/*.whl >> "$OUT/wheels/$name.log" 2>&1
}

build w0-nogeos ""
build w1-static-linemerge "geos-static"
build w2-static-wide "geos-static,geos-wide"
build w3-shared-linemerge "geos" PKG_CONFIG_PATH="$LIBDIR/pkgconfig" LD_LIBRARY_PATH="$LIBDIR"
build w4-shared-wide "geos,geos-wide" PKG_CONFIG_PATH="$LIBDIR/pkgconfig" LD_LIBRARY_PATH="$LIBDIR"

for d in "$OUT"/wheels/*/repaired; do
  for w in "$d"/*.whl; do
    echo "== $w $(stat -c %s "$w")"
    /opt/python/cp312-cp312/bin/python -c "import zipfile,sys; [print(i.file_size, i.compress_size, i.filename) for i in zipfile.ZipFile(sys.argv[1]).infolist() if i.filename.endswith(('.so',)) or '.so.' in i.filename]" "$w"
  done
done | tee "$OUT/wheel-sizes.txt"
