#!/usr/bin/env bash
# `just frames`: how many vblanks a panel's animation misses on this desktop's
# real GPU. Swaps the running bar for this checkout's Nova (scripts/run.sh,
# which puts the old bar back after), opens and closes a panel with
# `nova toggle` again and again, and reports the frames each screen dropped
# (scripts/frames-report.awk). Warns, and exits 1, when a screen drops more
# than BUDGET_PCT of its frames.
#
# `nova toggle` opens the panel on the focused screen only: focus another
# screen and run it again to measure that one. Windows behind the panel
# change what the compositor's blur costs, so compare runs on the same
# workspace.
#
# Usage: scripts/frames.sh [path/to/nova] [times] [panel]
#   times  how often to open and close the panel (default 20)
#   panel  which one, as `nova toggle` names it (default ai-usage)
set -euo pipefail
cd "$(dirname "$0")/.."

nova=$(realpath "${1:-target/debug/nova}")
times=${2:-20}
panel=${3:-ai-usage}
# Share of a screen's frames it may drop before this warns. Each dropped
# frame shows the one before for an extra refresh, which reads as a stutter.
BUDGET_PCT=2
# Between toggles: past the longest animation a panel runs (the AI usage
# meters fill until 750 ms after opening), so each toggle is its own burst.
TOGGLE_SECS=1.5
# How long Nova may take to start and the panel to become ready.
START_SECS=30
# What the bar logs once it has drawn itself (ui/bar/Bar.qml).
BAR_SHOWN="nova: bar shown"

# Each screen's refresh rate, as frames-report.awk takes them.
refresh_rates() {
	if [[ -n ${NIRI_SOCKET:-} ]]; then
		niri msg -j outputs | jq -r '.[] | select(.current_mode != null) | "\(.name)=\(.modes[.current_mode].refresh_rate / 1000)"'
	elif [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
		hyprctl -j monitors | jq -r '.[] | "\(.name)=\(.refreshRate)"'
	else
		echo "frames: needs niri or Hyprland, for each screen's refresh rate" >&2
		return 1
	fi
}
refresh=$(refresh_rates | tr '\n' ' ')

work=$(mktemp -d)
runner="" capture=""
stop() {
	[[ -n $capture ]] && kill "$capture" 2>/dev/null
	# scripts/run.sh puts the old bar back on TERM.
	if [[ -n $runner ]]; then
		kill -TERM "$runner" 2>/dev/null || true
		wait "$runner" 2>/dev/null || true
	fi
	capture="" runner=""
}
trap 'stop; rm -rf "$work"' EXIT

NOVA_FRAME_LOG=1 scripts/run.sh "$nova" >"$work/run.log" 2>&1 &
runner=$!

# The novad answering on the socket is this run's (scripts/run.sh's child),
# not the one it's replacing, and its UI has drawn the bar.
ready() {
	local pid
	pid=$("$nova" status --json 2>/dev/null | jq -r .pid) || return 1
	[[ $(awk '/^PPid:/ { print $2 }' "/proc/$pid/status" 2>/dev/null) == "$runner" ]] || return 1
	# Not grep -q: it stops reading early, which pipefail counts as failing.
	"$nova" logs --no-follow --ui -n 1000000 | grep -F "$BAR_SHOWN" >/dev/null
}
waited=0
until ready; do
	if ! kill -0 "$runner" 2>/dev/null || ((waited++ >= START_SECS * 2)); then
		echo "frames: Nova didn't start; its output:" >&2
		cat "$work/run.log" >&2
		exit 1
	fi
	sleep 0.5
done

# Followed live from novad, so its log rotating doesn't lose lines.
"$nova" logs --ui -n 0 >"$work/frames.log" &
capture=$!

# The first toggle also waits for the panel's feature to be ready.
waited=0
until "$nova" toggle "$panel" 2>"$work/toggle.err"; do
	if ((waited++ >= START_SECS)); then
		echo "frames: \`nova toggle $panel\` kept failing: $(<"$work/toggle.err")" >&2
		exit 1
	fi
	sleep 1
done
echo "frames: opening and closing $panel $times times on the focused screen"
sleep "$TOGGLE_SECS"
"$nova" toggle "$panel"
for ((i = 1; i < times; i++)); do
	sleep "$TOGGLE_SECS"
	"$nova" toggle "$panel"
	sleep "$TOGGLE_SECS"
	"$nova" toggle "$panel"
done
sleep "$TOGGLE_SECS"

# The old bar back before the report, which may exit 1.
stop
awk -v refresh="$refresh" -v budget="$BUDGET_PCT" -f scripts/frames-report.awk "$work/frames.log"
