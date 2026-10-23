#!/usr/bin/env bash
# Headless end-to-end run: starts sway with no real outputs and novad, which
# runs the Quickshell UI, then checks that
#   - novad reports the UI as running,
#   - the UI starts without QML errors,
#   - the UI connects to novad,
#   - memory (Pss) is within the budget: NOVA_PSS_BUDGET_MB per monitor
#     (default 250, so 500 MB for the default two monitors),
# and saves a screenshot to e2e/out/screenshot.png.
#
# The budget is a target, not a hard limit: going over prints a warning
# (a GitHub annotation in CI). Set NOVA_PSS_STRICT=1 to fail instead.
#
# Needs: sway, grim, jq, quickshell (or the nova-ui launcher), and a built
# nova binary
# (cargo build). Logs land in e2e/out; the UI's lines are in e2e/out/ui.log.
# Usage: e2e/run.sh [path/to/nova]
set -euo pipefail
cd "$(dirname "$0")/.."

nova=${1:-target/debug/nova}
monitors=${NOVA_E2E_MONITORS:-2}
budget=$((${NOVA_PSS_BUDGET_MB:-250} * monitors))
settle=${NOVA_E2E_SETTLE:-3}
out=e2e/out
rm -rf "$out" && mkdir -p "$out"

export XDG_RUNTIME_DIR=$(mktemp -d)
chmod 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export QT_QPA_PLATFORM=wayland
# No GPU in CI: render with Qt Quick's software backend.
export QT_QUICK_BACKEND=software
export NOVA_FEATURES_DIR=$PWD/features
# Nix's sway wrapper starts its own session bus when none is set, which fails
# on hosts without /etc/dbus-1. Nova doesn't need one yet.
export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-disabled:}
# The compositor here is sway, not the one this runs in: novad mustn't follow
# the session's niri or Hyprland.
unset NIRI_SOCKET HYPRLAND_INSTANCE_SIGNATURE

pids=()
cleanup() {
  # Newest first, so the UI and novad go before the compositor.
  for ((i = ${#pids[@]} - 1; i >= 0; i--)); do kill "${pids[i]}" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$XDG_RUNTIME_DIR"
}
trap cleanup EXIT

fail() {
  echo "e2e: FAIL: $*" >&2
  for log in "$out"/*.log "$out"/state/nova.log; do [[ -f $log ]] && { echo "--- $log"; tail -n 30 "$log"; }; done >&2
  exit 1
}

wait_for() { # <seconds> <description> <command...>
  local t=$1 what=$2; shift 2
  for _ in $(seq $((t * 10))); do "$@" && return 0; sleep 0.1; done
  fail "timed out waiting for $what"
}

export WLR_HEADLESS_OUTPUTS=$monitors
for i in $(seq "$monitors"); do
  printf 'output HEADLESS-%d resolution 1920x1080\n' "$i"
done >"$XDG_RUNTIME_DIR/sway.conf"
sway -c "$XDG_RUNTIME_DIR/sway.conf" >"$out/sway.log" 2>&1 &
pids+=($!)
wait_for 10 "sway" bash -c 'ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1'
WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -n1)")
export WAYLAND_DISPLAY

# Its own AI usage files, so the run never touches the user's or asks Anthropic.
"$nova" daemon --ui ui --log-dir "$out/state" --state-dir "$out/state" --codex-home "$out/codex" \
  --claude-credentials "$out/claude/.credentials.json" >"$out/daemon.log" 2>&1 &
pids+=($!)
wait_for 10 "novad socket" test -S "$XDG_RUNTIME_DIR/nova/novad.sock"
sleep "$settle"

"$nova" status --json >"$out/status.json" || fail "nova status failed"
# The UI's lines, as novad logged them, for the checks below.
grep ' ui: ' "$out/state/nova.log" >"$out/ui.log" || true
[[ $(jq -r .ui.state "$out/status.json") == running ]] || fail "novad doesn't report the UI as running"
[[ $(jq -r .ui.restarts "$out/status.json") == 0 ]] || fail "the UI crashed and was restarted"
ui=$(jq -r .ui.pid "$out/status.json")

if grep -E 'ERROR|TypeError|ReferenceError|is not a type|is not installed|Failed to load' "$out/ui.log"; then
  fail "QML errors in the UI log"
fi

grep -q 'nova: connected to novad' "$out/ui.log" || fail "the UI never connected to novad"

# ps and btop show the UI as NovaUI: argv[0] always, the process name when
# the nova-ui launcher is installed (the dev shell has it). The daemon is Nova.
[[ $(tr '\0' '\n' <"/proc/$ui/cmdline" | head -n1) == NovaUI ]] || fail "the UI's argv[0] isn't NovaUI"
if command -v nova-ui >/dev/null; then
  [[ $(<"/proc/$ui/comm") == NovaUI ]] || fail "the UI's process name isn't NovaUI"
fi
[[ $(<"/proc/$(jq -r .pid "$out/status.json")/comm") == Nova ]] || fail "the daemon's process name isn't Nova"

grim "$out/screenshot.png" || fail "screenshot failed"

pss_kb=$(awk '/^Pss:/ { print $2 }' "/proc/$ui/smaps_rollup")
pss_mb=$((pss_kb / 1024))
echo "e2e: UI Pss ${pss_mb} MB on ${monitors} monitors (target ${budget} MB)"
echo "$pss_mb" >"$out/pss_mb"
if ((pss_mb > budget)); then
  [[ ${NOVA_PSS_STRICT:-0} == 1 ]] && fail "UI uses ${pss_mb} MB, over the ${budget} MB target"
  echo "::warning title=Memory over target::UI uses ${pss_mb} MB on ${monitors} monitors, over the ${budget} MB target"
fi

echo "e2e: ok, screenshot in $out/screenshot.png"
