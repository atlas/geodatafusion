#!/usr/bin/env bash
# Run a command in the e3-ubuntu image (Ubuntu 24.04 + apt GEOS 3.12.1), with the repo at /work and
# the host's Rust toolchain and cargo registry mounted. Build with: podman build -t e3-ubuntu -f Containerfile .
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
exec podman run --rm \
  -v "$REPO":/work \
  -v "$HOME/.rustup":/root/.rustup:ro \
  -v "$HOME/.cargo/registry":/root/.cargo/registry \
  -e RUSTUP_TOOLCHAIN=1.97.1 \
  -e PATH=/root/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  -w /work \
  ${E3_PODMAN_ARGS:-} \
  localhost/e3-ubuntu "$@"
