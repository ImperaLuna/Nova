#!/usr/bin/env bash
# Fails when a Rust file in a crate is longer than MAX_LINES, as Nova Files'
# tools/check-file-length.sh does (AGENTS.md, Rules and their checks).
#
# Aim for modules under about 500 lines; past MAX_LINES a file must be split
# into smaller modules that each do one job.
# Usage: scripts/check-file-length.sh [root]
set -euo pipefail

cd "${1:-.}"
readonly MAX_LINES=800

status=0
while IFS= read -r -d '' file; do
  lines=$(wc -l <"$file")
  if ((lines > MAX_LINES)); then
    echo "$file: $lines lines (limit $MAX_LINES). Split it into smaller modules." >&2
    status=1
  fi
done < <(find app framework features fronts -name '*.rs' -print0)
exit "$status"
