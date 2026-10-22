#!/usr/bin/env bash
# Lints every QML file under ui/ and fails on any warning.
#
# Usage: scripts/qmllint.sh <qml import dir>...   (Quickshell's and Qt's qml dirs)
#
# One warning is filtered: qmllint checks a `pragma Singleton` file without
# reading the qmldir next to it, so it always claims the singleton isn't
# declared. Files that import the singleton do resolve it correctly.
set -euo pipefail
cd "$(dirname "$0")/../ui"

args=()
for dir in "$@"; do args+=(-I "$dir"); done

mapfile -t files < <(find . -name '*.qml' | sort)
out=$(qmllint "${args[@]}" "${files[@]}" 2>&1 || true)
out=$(grep -v 'not declared as singleton in qmldir but using pragma Singleton' <<<"$out" || true)
if [[ -n "$out" ]]; then
  echo "$out"
  exit 1
fi
echo "qmllint: ${#files[@]} files clean"
