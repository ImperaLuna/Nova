#!/usr/bin/env bash
# `just run`: swaps the running bar for this checkout's Nova. Ctrl+C, or
# closing the terminal, puts the old bar back as it was.
#
# - An installed Nova run by systemd (novad.service) is stopped, then started
#   again afterwards.
# - Any other bar is found by the basename of argv[0] (not comm, which Nix
#   wrappers truncate to names like ".quickshell-wra"), snapshotted (argv,
#   environment, working directory), killed, and relaunched from the snapshot
#   with `env -i`, so it doesn't inherit this shell's `nix develop` variables.
#   Only top-level matches are relaunched, so a supervisor isn't doubled.
#
# NOVA_RUN_REPLACE="waybar" picks which programs count as bars; empty
# replaces nothing.
#
# Usage: scripts/run.sh [path/to/novad]   (`just run` builds first)
set -euo pipefail
cd "$(dirname "$0")/.."

novad=${1:-target/debug/novad}
bars=${NOVA_RUN_REPLACE-"waybar quickshell qs nova-shell ags eww ironbar hyprpanel novad"}
unit=novad.service
# Dev runs log apart from the installed Nova, so its log history stays intact.
log_dir=${XDG_STATE_HOME:-$HOME/.local/state}/nova-dev
snapshots=$(mktemp -d)
restart_unit=0
restored=0

is_bar() { # <argv0 basename>
  local bar
  for bar in $bars; do [[ $1 == "$bar" ]] && return 0; done
  return 1
}

argv0_name() { # <pid>
  local argv0
  IFS= read -r -d '' argv0 <"/proc/$1/cmdline" 2>/dev/null || return 1
  basename -- "$argv0"
}

# Bars owned by this user, as "pid ppid" lines, skipping this script's own tree.
find_bars() {
  local dir pid ppid name
  for dir in /proc/[0-9]*; do
    pid=${dir#/proc/}
    [[ -O $dir && $pid != "$$" ]] || continue
    name=$(argv0_name "$pid") || continue
    is_bar "$name" || continue
    ppid=$(awk '/^PPid:/ { print $2 }' "$dir/status" 2>/dev/null) || continue
    echo "$pid $ppid"
  done
}

# Running, not a zombie waiting to be reaped.
alive() { # <pid>
  local state
  state=$(awk '/^State:/ { print $2 }' "/proc/$1/status" 2>/dev/null) && [[ $state != Z ]]
}

snapshot() { # <pid>
  local dir=$snapshots/$1
  mkdir "$dir"
  cp "/proc/$1/cmdline" "$dir/cmdline"
  cp "/proc/$1/environ" "$dir/environ"
  readlink "/proc/$1/cwd" >"$dir/cwd"
}

relaunch() { # <snapshot dir>
  local argv env_vars cwd
  mapfile -d '' argv <"$1/cmdline"
  mapfile -d '' env_vars <"$1/environ"
  cwd=$(<"$1/cwd")
  echo "run: restoring ${argv[*]}"
  # Started outside this terminal's scope, so it outlives the terminal;
  # setsid when there's no systemd user manager to ask.
  systemd-run --user --quiet --collect --working-directory="$cwd" \
    "$(command -v env)" -i "${env_vars[@]}" "${argv[@]}" >/dev/null 2>&1 ||
    (cd "$cwd" && setsid -f env -i "${env_vars[@]}" "${argv[@]}" >/dev/null 2>&1 </dev/null)
}

restore() {
  ((restored)) && return
  restored=1
  if [[ -n ${dev_pid:-} ]]; then
    kill "$dev_pid" 2>/dev/null || true
    wait "$dev_pid" 2>/dev/null || true
  fi
  local dir
  for dir in "$snapshots"/*/; do [[ -d $dir ]] && relaunch "$dir"; done
  if ((restart_unit)); then
    echo "run: starting $unit again"
    systemctl --user start "$unit"
  fi
  rm -rf "$snapshots"
}
trap restore EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

if systemctl --user is-active --quiet "$unit" 2>/dev/null; then
  echo "run: stopping the installed Nova ($unit)"
  systemctl --user stop "$unit"
  restart_unit=1
fi

mapfile -t found < <(find_bars)
declare -A matched=()
for line in "${found[@]}"; do matched[${line%% *}]=1; done
top_level=()
for line in "${found[@]}"; do
  read -r pid ppid <<<"$line"
  [[ -n ${matched[$ppid]:-} ]] || top_level+=("$pid")
done
for pid in "${top_level[@]}"; do
  snapshot "$pid"
  echo "run: replacing $(tr '\0' ' ' <"/proc/$pid/cmdline")"
done
# Top-level first, so a supervisor can't respawn its child.
for pid in "${top_level[@]}" "${!matched[@]}"; do kill "$pid" 2>/dev/null || true; done
for _ in $(seq 50); do
  alive=0
  for pid in "${!matched[@]}"; do alive "$pid" && alive=1; done
  ((alive)) || break
  sleep 0.1
done
for pid in "${!matched[@]}"; do kill -KILL "$pid" 2>/dev/null || true; done

echo "run: starting this checkout's Nova (logs below, Ctrl+C to stop)"
"$novad" --ui ui --features-dir features --log-dir "$log_dir" &
dev_pid=$!
wait "$dev_pid" || true
