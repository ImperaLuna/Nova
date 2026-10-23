#!/usr/bin/env bash
# `just run`: swaps the running bar for this checkout's Nova. Ctrl+C, or
# closing the terminal, puts the old bar back as it was.
#
# - An installed Nova run by systemd (nova.service) is stopped, then started
#   again afterwards.
# - Any other bar is found by the basename of argv[0] (not comm, which Nix
#   wrappers truncate to names like ".quickshell-wra"), snapshotted (argv,
#   environment, working directory), killed, and relaunched from the snapshot
#   with `env -i`, so it doesn't inherit this shell's `nix develop` variables.
#   Only top-level matches are relaunched, so a supervisor isn't doubled.
#   An entry like "nova:daemon" also needs argv[1] to match, so a Nova
#   daemon started without systemd is replaced but `nova logs` isn't.
#
# Only one `just run` at a time, across every checkout and worktree: a second
# one exits at once instead of swapping out the first one's Nova.
#
# NOVA_RUN_REPLACE="waybar" picks which programs count as bars; empty
# replaces nothing.
#
# Usage: scripts/run.sh [path/to/nova]   (`just run` builds first)
set -euo pipefail
cd "$(dirname "$0")/.."

nova=${1:-target/debug/nova}
bars=${NOVA_RUN_REPLACE-"waybar quickshell qs nova-shell ags eww ironbar hyprpanel Nova:daemon nova:daemon novad"}
unit=nova.service
# Dev runs log apart from the installed Nova, so its log history stays intact.
log_dir=${XDG_STATE_HOME:-$HOME/.local/state}/nova-dev
snapshots=$(mktemp -d)
restart_unit=0
restored=0

is_bar() { # <argv0 basename> <argv1>
  local bar
  for bar in $bars; do [[ $1 == "$bar" || "$1:$2" == "$bar" ]] && return 0; done
  return 1
}

# argv[0]'s basename and argv[1], one per line.
process_name() { # <pid>
  local argv
  mapfile -d '' -n 2 argv <"/proc/$1/cmdline" 2>/dev/null || return 1
  ((${#argv[@]})) || return 1
  basename -- "${argv[0]}"
  echo "${argv[1]:-}"
}

# Bars owned by this user, as "pid ppid" lines, skipping this script's own tree.
find_bars() {
  local dir pid ppid name
  for dir in /proc/[0-9]*; do
    pid=${dir#/proc/}
    [[ -O $dir && $pid != "$$" ]] || continue
    mapfile -t name < <(process_name "$pid") || continue
    ((${#name[@]})) || continue
    is_bar "${name[0]}" "${name[1]:-}" || continue
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
  readlink "/proc/$1/exe" >"$dir/exe"
}

relaunch() { # <snapshot dir>
  local argv env_vars cwd exe
  mapfile -d '' argv <"$1/cmdline"
  mapfile -d '' env_vars <"$1/environ"
  cwd=$(<"$1/cwd")
  # The executable itself: argv[0] may be just a name (Nova, NovaUI). If it
  # was replaced since (an upgrade), argv[0] is the best guess left.
  exe=$(<"$1/exe")
  [[ -x $exe ]] || exe=${argv[0]}
  echo "run: restoring ${argv[*]}"
  # Runs `exe` with the original argv[0]; env -i leaves no PATH, so bash is
  # named by its full path.
  local launch=("$(command -v env)" -i "${env_vars[@]}" "$BASH" -c 'exec -a "$0" "$@"' "${argv[0]}" "$exe" "${argv[@]:1}")
  # Started outside this terminal's scope, so it outlives the terminal;
  # setsid when there's no systemd user manager to ask. KillMode=process:
  # apps launched from the bar can stay in its cgroup, and the next run
  # stopping the bar mustn't take them down with it.
  systemd-run --user --quiet --collect -p KillMode=process --working-directory="$cwd" "${launch[@]}" >/dev/null 2>&1 ||
    (cd "$cwd" && setsid -f "${launch[@]}" >/dev/null 2>&1 </dev/null 9>&-)
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
# Held until this script exits. The bars and Nova it starts don't inherit it.
lock=${XDG_RUNTIME_DIR:-/tmp}/nova-run.lock
exec 9>"$lock"
if ! flock -n 9; then
  echo "run: another \`just run\` is using the bar; wait for it to stop" >&2
  rm -rf "$snapshots"
  exit 1
fi

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
# argv[0] Nova, as the systemd unit and `nova` start it.
(exec -a Nova "$nova" daemon --ui ui --features-dir features --log-dir "$log_dir" 9>&-) &
dev_pid=$!
wait "$dev_pid" || true
