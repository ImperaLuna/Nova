#!/usr/bin/env bash
# Compiles every fragment shader under ui/ (*.frag) into the .qsb next to it,
# which is what QML loads. The .qsb files are committed, since ui/ ships as
# it is; with --check, fails if any of them differs from a fresh build.
#
# Usage: scripts/shaders.sh [--check]   (needs qsb, from Qt Shader Tools)
set -euo pipefail
cd "$(dirname "$0")/../ui"

check=false
[[ ${1:-} == --check ]] && check=true

# Every backend Quickshell's Qt can pick: GLSL for OpenGL (ES), SPIR-V for
# Vulkan (always included), HLSL and MSL for completeness.
targets=(--glsl "100 es,120,150" --hlsl 50 --msl 12)

status=0
while IFS= read -r frag; do
  if $check; then
    fresh=$(mktemp)
    qsb "${targets[@]}" -o "$fresh" "$frag"
    if ! cmp -s "$fresh" "$frag.qsb"; then
      echo "$frag.qsb is out of date: run scripts/shaders.sh" >&2
      status=1
    fi
    rm -f "$fresh"
  else
    qsb "${targets[@]}" -o "$frag.qsb" "$frag"
    echo "compiled $frag"
  fi
done < <(find . -name '*.frag' | sort)
exit $status
