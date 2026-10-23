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

# Bounds on the release download (AGENTS.md: network calls have a timeout,
# a size cap and back off). The archive is a few MB.
download_limits="--connect-timeout 15 --max-time 300 --max-filesize 100M --retry 3"

# Unpacks the release archive (`nix build .#dist`, published by release.yml)
# into $work/release: bin/nova and share/nova, laid out like the prefix.
# Fails when there's no archive for this machine.
fetch_release() {
	command -v curl >/dev/null || return 1
	echo "Fetching Nova from $release_url"
	# shellcheck disable=SC2086 # the limits are separate words
	curl -fsSL $download_limits "$release_url" -o "$work/nova.tar.gz" 2>/dev/null || return 1
	mkdir -p "$work/release"
	tar -xzf "$work/nova.tar.gz" -C "$work/release" || fail "the release archive is damaged."
	# Owner-writable, so a later install can replace share/nova.
	chmod -R u+w "$work/release"
	[ -x "$work/release/bin/nova" ] || fail "the release archive has no bin/nova."
}

# Builds the same tree from source into $work/tree.
build_source() {
	command -v git >/dev/null || fail "no release fits this machine, and building Nova needs git."
	command -v cargo >/dev/null || fail "no release fits this machine, and building Nova needs Rust's cargo (rustup.rs)."
	echo "No release fits this machine; building Nova from source."
	git clone --quiet --depth 1 "$repo" "$work/src"
	cargo build --release --locked --manifest-path "$work/src/Cargo.toml" --target-dir "$work/target" --bin nova
	mkdir -p "$work/tree/bin" "$work/tree/share/nova"
	cp "$work/target/release/nova" "$work/tree/bin/nova"
	for dir in features themes ui; do
		cp -R "$work/src/$dir" "$work/tree/share/nova/$dir"
	done
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

# Only the download is a condition: the install itself runs under `set -e`.
if fetch_release; then
	install_tree "$work/release"
else
	build_source
	install_tree "$work/tree"
fi
echo "Installed $prefix/bin/nova."
case ":$PATH:" in
*":$prefix/bin:"*) ;;
*) echo "Add $prefix/bin to your PATH to run nova from a terminal." ;;
esac
# exec skips the EXIT trap, so the download or source build goes first.
rm -rf "$work"
trap - EXIT
# Through `curl | sh`, stdin is this script: setup asks on the terminal.
if [ ! -t 0 ] && (exec </dev/tty) 2>/dev/null; then
	exec "$prefix/bin/nova" setup "$@" </dev/tty
fi
exec "$prefix/bin/nova" setup "$@"
