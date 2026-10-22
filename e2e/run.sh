#!/usr/bin/env bash
# Headless end-to-end run: starts sway with no real outputs, novad and the
# Quickshell UI, then checks that
#   - the UI starts without QML errors,
#   - the UI connects to novad,
#   - memory (Pss) stays under NOVA_PSS_BUDGET_MB (default 250),
# and saves a screenshot to e2e/out/screenshot.png.
#
# Needs: sway, grim, quickshell (qs), and a built novad (cargo build).
# Usage: e2e/run.sh [path/to/novad]
set -euo pipefail
cd "$(dirname "$0")/.."

novad=${1:-target/debug/novad}
budget=${NOVA_PSS_BUDGET_MB:-250}
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
  for log in "$out"/*.log; do echo "--- $log"; tail -n 30 "$log"; done >&2
  exit 1
}

wait_for() { # <seconds> <description> <command...>
  local t=$1 what=$2; shift 2
  for _ in $(seq $((t * 10))); do "$@" && return 0; sleep 0.1; done
  fail "timed out waiting for $what"
}

printf 'output HEADLESS-1 resolution 1920x1080\n' >"$XDG_RUNTIME_DIR/sway.conf"
sway -c "$XDG_RUNTIME_DIR/sway.conf" >"$out/sway.log" 2>&1 &
pids+=($!)
wait_for 10 "sway" bash -c 'ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1'
WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -n1)")
export WAYLAND_DISPLAY

"$novad" >"$out/novad.log" 2>&1 &
pids+=($!)
wait_for 10 "novad socket" test -S "$XDG_RUNTIME_DIR/nova/novad.sock"

qs -p ui >"$out/ui.log" 2>&1 &
ui=$!
pids+=($ui)
sleep "$settle"
kill -0 "$ui" 2>/dev/null || fail "the UI exited"

if grep -E 'ERROR|TypeError|ReferenceError|is not a type|is not installed|Failed to load' "$out/ui.log"; then
  fail "QML errors in the UI log"
fi

grep -q 'nova: connected to novad' "$out/ui.log" || fail "the UI never connected to novad"

grim "$out/screenshot.png" || fail "screenshot failed"

pss_kb=$(awk '/^Pss:/ { print $2 }' "/proc/$ui/smaps_rollup")
pss_mb=$((pss_kb / 1024))
echo "e2e: UI Pss ${pss_mb} MB (budget ${budget} MB)"
echo "$pss_mb" >"$out/pss_mb"
((pss_mb <= budget)) || fail "UI uses ${pss_mb} MB, over the ${budget} MB budget"

echo "e2e: ok, screenshot in $out/screenshot.png"
