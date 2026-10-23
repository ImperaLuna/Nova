#!/bin/sh
# Installs Nova for this user, then runs `nova setup`, which does the rest
# (packages, config, the systemd service). Everything past fetching the
# binary is in Rust, so this stays small.
#
#   curl -fsSL https://raw.githubusercontent.com/ImperaLuna/Nova/main/install.sh | sh
#
# Nova lands in ~/.local: bin/nova, plus share/nova (the UI, features and
# themes), which is where nova looks next to its binary. It comes from the
# latest release when there's a build for this machine, else it's built from
# source with cargo. Arguments are passed on to `nova setup` (--dry-run,
# --yes).
#
# NOVA_PREFIX installs somewhere else; NOVA_OS_RELEASE and NOVA_RELEASE_URL
# are for tests.
set -eu

repo="https://github.com/ImperaLuna/Nova"
prefix="${NOVA_PREFIX:-$HOME/.local}"
os_release="${NOVA_OS_RELEASE:-/etc/os-release}"

fail() {
	echo "install.sh: $*" >&2
	exit 1
}

[ "$(uname -s)" = Linux ] || fail "Nova runs on Linux only."
case "$(uname -m)" in
x86_64 | aarch64) arch="$(uname -m)" ;;
*) fail "there's no Nova for $(uname -m); x86_64 and aarch64 are supported." ;;
esac
release_url="${NOVA_RELEASE_URL:-$repo/releases/latest/download/nova-$arch-linux.tar.gz}"

if [ -r "$os_release" ] && grep -qx 'ID=nixos' "$os_release"; then
	fail "on NixOS, install Nova with its flake ($repo); there's nothing for this script to do."
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# A release archive (`nix build .#dist`, published by release.yml) holds
# bin/nova and share/nova, laid out like the prefix.
from_release() {
	command -v curl >/dev/null || return 1
	echo "Fetching Nova from $release_url"
	curl -fsSL "$release_url" -o "$work/nova.tar.gz" 2>/dev/null || return 1
	mkdir -p "$work/release"
	tar -xzf "$work/nova.tar.gz" -C "$work/release"
	[ -x "$work/release/bin/nova" ] || fail "the release archive has no bin/nova."
	install_tree "$work/release"
}

from_source() {
	command -v git >/dev/null || fail "no release fits this machine, and building Nova needs git."
	command -v cargo >/dev/null || fail "no release fits this machine, and building Nova needs Rust's cargo (rustup.rs)."
	echo "No release fits this machine; building Nova from source."
	git clone --quiet --depth 1 "$repo" "$work/src"
	cargo build --release --locked --manifest-path "$work/src/Cargo.toml" --bin nova
	mkdir -p "$work/tree/bin" "$work/tree/share/nova"
	cp "$work/src/target/release/nova" "$work/tree/bin/nova"
	for dir in features themes ui; do
		cp -R "$work/src/$dir" "$work/tree/share/nova/$dir"
	done
	install_tree "$work/tree"
}

# Copies a tree with bin/nova and share/nova into the prefix, replacing an
# older share/nova so removed files don't linger.
install_tree() {
	mkdir -p "$prefix/bin" "$prefix/share"
	rm -rf "$prefix/share/nova"
	cp -R "$1/share/nova" "$prefix/share/nova"
	cp "$1/bin/nova" "$prefix/bin/nova.new"
	chmod 755 "$prefix/bin/nova.new"
	mv "$prefix/bin/nova.new" "$prefix/bin/nova"
}

from_release || from_source
echo "Installed $prefix/bin/nova."
case ":$PATH:" in
*":$prefix/bin:"*) ;;
*) echo "Add $prefix/bin to your PATH to run nova from a terminal." ;;
esac
# Through `curl | sh`, stdin is this script: setup asks on the terminal.
if [ ! -t 0 ] && (exec </dev/tty) 2>/dev/null; then
	exec "$prefix/bin/nova" setup "$@" </dev/tty
fi
exec "$prefix/bin/nova" setup "$@"
