#!/usr/bin/env bash
# Compares a VM test's screenshots with the approved ones (e2e/golden/<compositor>)
# using odiff. A screenshot fails when more than MAX_DIFF_PERCENT of its
# pixels differ by more than COLOR_THRESHOLD; software rendering in a VM can
# differ slightly between runs, so neither is zero.
#
# Writes report.txt and, for each failed screenshot, diff/<state>.png (the
# changed pixels in red). Exits 1 when any screenshot fails or has no golden.
# Usage: compare.sh <screenshots dir> <golden dir> <out dir>
set -euo pipefail

COLOR_THRESHOLD=0.1
MAX_DIFF_PERCENT=0.01

shots=$1 golden=$2 out=$3
mkdir -p "$out/diff"
report=$out/report.txt
failed=0

for shot in "$shots"/*.png; do
  state=$(basename "$shot" .png)
  if [[ ! -f $golden/$state.png ]]; then
    echo "$state: no golden screenshot; approve one with \`just e2e-approve\`" >>"$report"
    failed=1
    continue
  fi
  # Prints "<pixels>;<percent>" when the images differ, nothing when they match.
  result=$(odiff --parsable-stdout --antialiasing --threshold "$COLOR_THRESHOLD" \
    "$golden/$state.png" "$shot" "$out/diff/$state.png" || true)
  percent=${result#*;}
  if [[ -n $result ]] && awk -v p="$percent" -v max="$MAX_DIFF_PERCENT" 'BEGIN { exit !(p > max) }'; then
    echo "$state: ${percent}% of pixels differ (${result%;*} pixels; at most ${MAX_DIFF_PERCENT}% allowed)" >>"$report"
    failed=1
  else
    echo "$state: ok (${percent:-0}% differ)" >>"$report"
    rm -f "$out/diff/$state.png"
  fi
done

cat "$report"
exit "$failed"
