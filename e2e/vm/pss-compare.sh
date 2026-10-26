#!/usr/bin/env bash
# Compares the Pss of each state (frames.py's pss.txt) between main and a PR,
# for one compositor. Prints a Markdown table for the job summary, and a
# GitHub warning for each state whose UI Pss grew by more than
# GROWTH_WARN_MB. VM numbers move a little between runs, so small changes
# are noise. A state missing on one side shows a dash.
# Usage: pss-compare.sh <compositor> <main pss.txt> <PR pss.txt>
set -euo pipefail

GROWTH_WARN_MB=10

compositor=$1 main=$2 pr=$3

# "<state>: UI <mb> MB, daemon <mb> MB" -> "<state>\t<ui>\t<daemon>"
fields() {
  sed -nE 's/^(.+): UI ([0-9.]+) MB, daemon ([0-9.]+) MB$/\1\t\2\t\3/p' "$1"
}

echo "#### Pss on $compositor (MB, main → PR)"
echo
echo "| State | UI | Daemon |"
echo "| --- | --- | --- |"
awk -F '\t' -v compositor="$compositor" -v warn="$GROWTH_WARN_MB" '
  function cell(before, after) {
    if (before == "" || after == "") return (before == "" ? "–" : before) " → " (after == "" ? "–" : after)
    return sprintf("%s → %s (%+.1f)", before, after, after - before)
  }
  FNR == NR { ui[$1] = $2; daemon[$1] = $3; next }
  {
    seen[$1] = 1
    printf "| %s | %s | %s |\n", $1, cell(ui[$1], $2), cell(daemon[$1], $3)
    if ($1 in ui && $2 - ui[$1] > warn)
      printf "::warning title=Pss grew on %s::UI Pss with %s grew %.1f MB against main (%s -> %s MB)\n", \
        compositor, $1, $2 - ui[$1], ui[$1], $2 > "/dev/stderr"
  }
  END { for (state in ui) if (!(state in seen)) printf "| %s | %s | %s |\n", state, cell(ui[state], ""), cell(daemon[state], "") }
' <(fields "$main") <(fields "$pr")
