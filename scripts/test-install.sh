#!/usr/bin/env bash
# Tests install.sh without the network: a fake release archive whose
# `nova` records how it was run, a fake prefix, and a fake NixOS.
set -euo pipefail

script="$(realpath "${1:-install.sh}")"
work="$(mktemp -d)"
trap 'chmod -R u+w "$work"; rm -rf "$work"' EXIT

mkdir -p "$work/release/bin" "$work/release/share/nova/themes"
# shellcheck disable=SC2016 # $0 and $* belong to the fake nova.
printf '#!/bin/sh\necho "$0 $*" > "%s/ran"\n' "$work" >"$work/release/bin/nova"
chmod +x "$work/release/bin/nova"
touch "$work/release/share/nova/themes/default.toml"
# Read-only like the Nix store, so the upgrade below proves the installed
# tree stays replaceable.
chmod -R a-w "$work/release"
# The archive and its checksum, as `nix build .#dist` names them.
pack() {
	tar -czf "$work/nova.tar.gz" -C "$work/release" bin share
	(cd "$work" && sha256sum nova.tar.gz >nova.tar.gz.sha256)
}
pack
echo 'ID=arch' >"$work/os-release"

# install.sh's own temporary files go under $work/tmp, to check it removes them.
run() {
	mkdir -p "$work/tmp"
	TMPDIR="$work/tmp" NOVA_PREFIX="$work/prefix" NOVA_OS_RELEASE="$work/os-release" \
		NOVA_RELEASE_URL="file://$work/nova.tar.gz" sh "$script" "$@" </dev/null
}

# A file left from an older install goes; the new tree is in place.
mkdir -p "$work/prefix/share/nova/old"
run --dry-run >/dev/null
[ -x "$work/prefix/bin/nova" ] || { echo "no bin/nova" >&2; exit 1; }
[ -f "$work/prefix/share/nova/themes/default.toml" ] || { echo "no share/nova/themes" >&2; exit 1; }
[ -z "$(ls -A "$work/tmp")" ] || { echo "temporary files left behind" >&2; exit 1; }
[ ! -e "$work/prefix/share/nova/old" ] || { echo "old files linger" >&2; exit 1; }
[ "$(cat "$work/ran")" = "$work/prefix/bin/nova setup --dry-run" ] || { echo "ran: $(cat "$work/ran")" >&2; exit 1; }

# A newer release replaces the installed (read-only in the archive) tree.
chmod -R u+w "$work/release"
touch "$work/release/share/nova/themes/new.toml"
chmod -R a-w "$work/release"
pack
run --dry-run >/dev/null
[ -f "$work/prefix/share/nova/themes/new.toml" ] || { echo "the upgrade didn't replace share/nova" >&2; exit 1; }

# An archive that doesn't match its checksum, or has none, is never installed.
rm "$work/ran"
echo junk >>"$work/nova.tar.gz"
if out="$(run --dry-run 2>&1)"; then
	echo "installed an archive that doesn't match its checksum" >&2
	exit 1
fi
grep -q checksum <<<"$out" || { echo "no checksum error: $out" >&2; exit 1; }
rm "$work/nova.tar.gz.sha256"
if out="$(run --dry-run 2>&1)"; then
	echo "installed an archive without a checksum" >&2
	exit 1
fi
grep -q checksum <<<"$out" || { echo "no checksum error: $out" >&2; exit 1; }
[ ! -e "$work/ran" ] || { echo "nova setup ran after a failed check" >&2; exit 1; }
pack

echo 'ID=nixos' >"$work/os-release"
if out="$(run 2>&1)"; then
	echo "installed on NixOS" >&2
	exit 1
fi
grep -q flake <<<"$out" || { echo "no flake hint: $out" >&2; exit 1; }
echo "install.sh: ok"
