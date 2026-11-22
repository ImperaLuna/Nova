#!/usr/bin/env bash
# Step 4 build: copies the pinned fork from cargo's git cache into
# ../.scratch/imp-369/zed (git-ignored scratch, outside this workspace so
# cargo does not adopt its crates), applies fork-patch/custom-shader.patch
# and builds nova-proto with --features shader against it into
# target/shader. Run from anywhere inside `nix develop ./prototype`.
set -euo pipefail
cd "$(dirname "$0")/.."
rev=939cf37
fork="$(cd .. && pwd)/.scratch/imp-369/zed"
if [ ! -d "$fork" ]; then
  cargo fetch
  src=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/git/checkouts/zed-*/"$rev" | head -1)
  mkdir -p "$(dirname "$fork")"
  cp -r "$src" "$fork"
  chmod -R u+w "$fork"
  # The fork's toolchain file would make rustup fetch another toolchain.
  rm -f "$fork/rust-toolchain.toml" "$fork/rust-toolchain"
  git -C "$fork" apply "$PWD/fork-patch/custom-shader.patch"
fi
patch='patch."https://github.com/ImperaLuna/zed"'
cargo build --release --features shader \
  --target-dir target/shader \
  --config "$patch.gpui.path=\"$fork/crates/gpui\"" \
  --config "$patch.gpui_wgpu.path=\"$fork/crates/gpui_wgpu\"" \
  --config "$patch.gpui_linux.path=\"$fork/crates/gpui_linux\"" \
  --config "$patch.gpui_platform.path=\"$fork/crates/gpui_platform\""
echo "built target/shader/release/nova-proto"
