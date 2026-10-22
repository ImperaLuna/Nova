#!/usr/bin/env bash
# Formats every QML file under ui/ with qmlformat.
#
# Usage: scripts/qmlformat.sh          rewrite files in place
#        scripts/qmlformat.sh --check  fail and show a diff if any file isn't formatted
set -euo pipefail
cd "$(dirname "$0")/../ui"

mapfile -t files < <(find . -name '*.qml' | sort)
if [[ ${1:-} != --check ]]; then
  qmlformat -i "${files[@]}"
  exit 0
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
status=0
for f in "${files[@]}"; do
  qmlformat "$f" >"$tmp"
  if ! diff -u --label "$f" --label "$f (formatted)" "$f" "$tmp"; then
    status=1
  fi
done
((status == 0)) && echo "qmlformat: ${#files[@]} files formatted"
exit "$status"
