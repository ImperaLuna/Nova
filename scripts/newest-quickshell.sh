#!/usr/bin/env bash
# Builds Nova on the newest stable Quickshell release and runs its qmllint
# there (legacyPackages.<system>.newest-quickshell in flake.nix), then
# says whether Nova passes and whether that release is in the range
# packaging/quickshell.toml says Nova is tested with. A release that passes
# but is outside the range means `max` can be raised.
#
# Exits non-zero when Nova fails on the release (or it doesn't build with
# nixpkgs' Quickshell recipe). In GitHub Actions the result also goes to the
# job summary and an annotation.
#
# Usage: scripts/newest-quickshell.sh
set -euo pipefail
cd "$(dirname "$0")/.."

repo=https://github.com/quickshell-mirror/quickshell

# Stable releases are tagged vX.Y.Z.
tag=$(git ls-remote --tags --refs "$repo" 'v*' | sed 's|.*refs/tags/||' |
  grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1)
if [[ -z $tag ]]; then
  echo "No Quickshell release found at $repo" >&2
  exit 1
fi

system=$(nix eval --raw --impure --expr builtins.currentSystem)
attr=".#legacyPackages.$system.newest-quickshell"
flags=(--no-write-lock-file --override-input quickshell-newest "git+$repo?ref=refs/tags/$tag&shallow=1")
version=$(nix eval --raw "${flags[@]}" "$attr.quickshell.version")
in_range=$(nix eval "${flags[@]}" "$attr.inRange")
# shellcheck disable=SC2016 # Nix's ${}, not the shell's
range=$(nix eval --raw --impure --expr \
  'let r = builtins.fromTOML (builtins.readFile ./packaging/quickshell.toml); in "${r.min} to ${r.max}"')

passed=true
nix build -L --no-link --keep-going "${flags[@]}" "$attr.nova" "$attr.qmllint" || passed=false

if ! $passed; then
  level=error
  message="Nova fails on Quickshell $version, the newest release (tested range: $range)."
elif [[ $in_range != true ]]; then
  level=warning
  message="Nova passes on Quickshell $version, the newest release, which is outside the tested range ($range): raise max in packaging/quickshell.toml."
else
  level=notice
  message="Nova passes on Quickshell $version, the newest release (tested range: $range)."
fi

echo "$message"
if [[ -n ${GITHUB_ACTIONS:-} ]]; then
  echo "::$level::$message"
  echo "$message" >>"$GITHUB_STEP_SUMMARY"
fi
$passed
