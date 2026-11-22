#!/usr/bin/env bash
# Steps 4 and 5 build: copies the pinned fork from cargo's git cache into
# ../.scratch/imp-369/zed (git-ignored scratch, outside this workspace so
# cargo does not adopt its crates), applies fork-patch/*.patch (custom shader,
# session lock) and builds nova-proto with --features patched against it
# into target/patched. Run from anywhere inside `nix develop ./prototype`.
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
  for patch_file in fork-patch/*.patch; do
    git -C "$fork" apply "$PWD/$patch_file"
  done
fi
patch='patch."https://github.com/ImperaLuna/zed"'
cargo build --release --features patched \
  --target-dir target/patched \
  --config "$patch.gpui.path=\"$fork/crates/gpui\"" \
  --config "$patch.gpui_wgpu.path=\"$fork/crates/gpui_wgpu\"" \
  --config "$patch.gpui_linux.path=\"$fork/crates/gpui_linux\"" \
  --config "$patch.gpui_platform.path=\"$fork/crates/gpui_platform\""
echo "built target/patched/release/nova-proto"
