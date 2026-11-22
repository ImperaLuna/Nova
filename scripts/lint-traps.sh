#!/usr/bin/env bash
# Fails on test and script patterns that flaked before, each with its fix:
#   - `grep -q` (or --quiet) reading a pipe in a script that sets pipefail:
#     grep exits at its first match, the writer gets SIGPIPE, and pipefail
#     turns that into a failure (e2e-headless, the Nova Files batch). Send
#     grep's output to /dev/null instead.
#   - a novad test asserting "nothing came" with `try_recv().is_err()` or
#     `try_iter().any(...)`: other events of the same topics may well come
#     (a watcher starting, an index built). Count the one event that mustn't
#     with `hub::count_named`. This catches the common forms, not every
#     way to write it.
# Usage: scripts/lint-traps.sh [root]
set -euo pipefail

cd "${1:-.}"
failed=0

# report <what> <fix> <grep output>: prints the hits, if any, and fails.
report() {
  [[ -n $3 ]] || return 0
  printf '%s (%s):\n%s\n' "$1" "$2" "$3" >&2
  failed=1
}

# Every text file there, since some scripts have no .sh (scripts/desktop).
mapfile -t pipefail_scripts < <(grep -rlIE 'set -[a-z]*o pipefail' scripts e2e packaging 2>/dev/null || true)
if ((${#pipefail_scripts[@]})); then
  report "grep -q reading a pipe under pipefail" "use grep ... >/dev/null" \
    "$(grep -HnE '\|[[:space:]]*grep([[:space:]]+-[a-zA-Z-]+)*[[:space:]]+(-[a-zA-Z]*q[a-zA-Z]*|--quiet)([[:space:]]|$)' "${pipefail_scripts[@]}" || true)"
fi

report "a novad test asserting that no event came at all" "use hub::count_named for the event that mustn't come" \
  "$(grep -rnE 'try_recv\(\)\.is_err\(\)|try_iter\(\)\.any\(' app/src framework features || true)"

if ((failed)); then exit 1; fi
echo "lint-traps: nothing found"
