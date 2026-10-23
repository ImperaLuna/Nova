#!/usr/bin/env bash
# Tests install.sh without the network: a fake release archive whose
# `nova` records how it was run, a fake prefix, and a fake NixOS.
set -euo pipefail

script="$(realpath "${1:-install.sh}")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/release/bin" "$work/release/share/nova/ui"
# shellcheck disable=SC2016 # $0 and $* belong to the fake nova.
printf '#!/bin/sh\necho "$0 $*" > "%s/ran"\n' "$work" >"$work/release/bin/nova"
chmod +x "$work/release/bin/nova"
touch "$work/release/share/nova/ui/shell.qml"
tar -czf "$work/nova.tar.gz" -C "$work/release" bin share
echo 'ID=arch' >"$work/os-release"

run() {
	NOVA_PREFIX="$work/prefix" NOVA_OS_RELEASE="$work/os-release" \
		NOVA_RELEASE_URL="file://$work/nova.tar.gz" sh "$script" "$@" </dev/null
}

# A file left from an older install goes; the new tree is in place.
mkdir -p "$work/prefix/share/nova/old"
run --dry-run >/dev/null
[ -x "$work/prefix/bin/nova" ] || { echo "no bin/nova" >&2; exit 1; }
[ -f "$work/prefix/share/nova/ui/shell.qml" ] || { echo "no share/nova/ui" >&2; exit 1; }
[ ! -e "$work/prefix/share/nova/old" ] || { echo "old files linger" >&2; exit 1; }
[ "$(cat "$work/ran")" = "$work/prefix/bin/nova setup --dry-run" ] || { echo "ran: $(cat "$work/ran")" >&2; exit 1; }

echo 'ID=nixos' >"$work/os-release"
if out="$(run 2>&1)"; then
	echo "installed on NixOS" >&2
	exit 1
fi
grep -q flake <<<"$out" || { echo "no flake hint: $out" >&2; exit 1; }
echo "install.sh: ok"
