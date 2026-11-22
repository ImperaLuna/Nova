#!/usr/bin/env bash
# Tests scripts/check-layout.sh on throwaway workspaces: one in the layout
# that passes, then the same workspace with one planted mistake per case,
# each of which must fail with its rule id in the output.
# Needs cargo and jq; no network.
set -euo pipefail

script=$(realpath "$(dirname "$0")/check-layout.sh")
failed=0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# crate <workspace> <folder> <name> <allowed outside> <allowed workspace> [dependency line...]
# writes a library crate. The allowed lists are space-separated names, or
# "none"; each dependency line goes under [dependencies] as it is.
crate() {
  local workspace=$1 folder=$2 name=$3 outside=$4 inside=$5
  shift 5
  mkdir -p "$workspace/$folder/src"
  touch "$workspace/$folder/src/lib.rs"
  {
    allowed_header outside "$outside"
    echo "#"
    allowed_header workspace "$inside"
    printf '\n[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2024"\npublish = false\n\n[dependencies]\n' "$name"
    printf '%s\n' "$@"
  } >"$workspace/$folder/Cargo.toml"
}

# ui_crate <workspace> <folder> ...: a crate of the UI side, written like
# `crate` does and naming the UI side's workspace (app-ui/) as its own.
ui_crate() {
  crate "$@"
  local up=${2//[^\/]/}
  sed -i "s|^edition = .*|&\nworkspace = \"${up//\//../}../app-ui\"|" "$1/$2/Cargo.toml"
}

# allowed_header <outside|workspace> <names, or none>
allowed_header() {
  if [[ $2 == none ]]; then
    echo "# Allowed $1 crates: none"
    return
  fi
  echo "# Allowed $1 crates:"
  local each
  for each in $2; do
    echo "# - $each"
  done
}

# The tables both workspaces carry (rule S1).
shared_tables='
[workspace.package]
edition = "2024"

[workspace.lints.clippy]
unwrap_used = "deny"'

# good <workspace>: a small repo with every folder kind, in the layout: the
# daemon side's workspace at its root and the UI side's at app-ui/.
good() {
  local workspace=$1
  mkdir -p "$workspace/features/media" "$workspace/features/notifications"
  touch "$workspace/features/media/feature.toml" "$workspace/features/notifications/feature.toml"
  cat >"$workspace/Cargo.toml" <<EOF
[workspace]
resolver = "3"
members = ["app", "framework/*", "features/*/api", "features/*/core", "fronts/*"]
$shared_tables
EOF
  crate "$workspace" framework/carrier carrier none none
  ui_crate "$workspace" framework-ui/look look none carrier 'carrier = { path = "../../framework/carrier" }'
  ui_crate "$workspace" features/media/ui media-ui none "media-api look" \
    'media-api = { path = "../api" }' 'look = { path = "../../../framework-ui/look" }'
  # The UI program is its workspace's root. The members are named one by
  # one, so a case can add a crate that no workspace lists.
  crate "$workspace" app-ui nova-ui gpui "media-ui look" 'gpui = "1"' \
    'media-ui = { path = "../features/media/ui" }' 'look = { path = "../framework-ui/look" }' \
    '' '[workspace]' 'resolver = "3"' 'members = [".", "../framework-ui/look", "../features/media/ui"]' "$shared_tables"
  crate "$workspace" framework/paths paths none none
  crate "$workspace" framework/stage stage none paths 'paths = { path = "../paths" }'
  crate "$workspace" features/notifications/api notifications-api none none
  crate "$workspace" features/media/api media-api none notifications-api \
    'notifications-api = { path = "../../notifications/api" }'
  crate "$workspace" features/media/core media-core none "media-api notifications-api" \
    'media-api = { path = "../api" }' 'notifications-api = { path = "../../notifications/api" }'
  crate "$workspace" fronts/cli cli none media-api 'media-api = { path = "../../features/media/api" }'
  crate "$workspace" app app none "cli stage media-core" \
    'cli = { path = "../fronts/cli" }' 'stage = { path = "../framework/stage" }' \
    'media-core = { path = "../features/media/core" }'
}

# expect_pass <what> <workspace>
expect_pass() {
  local output
  if ! output=$("$script" "$2" 2>&1); then
    printf 'FAIL: %s should pass, got:\n%s\n' "$1" "$output" >&2
    failed=1
  fi
}

# expect_fail <what> <rule id> <workspace>
expect_fail() {
  local output
  if output=$("$script" "$3" 2>&1); then
    printf 'FAIL: %s should fail with rule %s, but passed\n' "$1" "$2" >&2
    failed=1
  elif [[ $output != *"rule $2"* ]]; then
    printf 'FAIL: %s should fail with rule %s, got:\n%s\n' "$1" "$2" "$output" >&2
    failed=1
  fi
}

# planted <case>: a fresh good workspace to plant one mistake in.
planted() {
  local workspace=$work/$1
  good "$workspace"
  echo "$workspace"
}

expect_pass "the good workspace" "$(planted good)"

ws=$(planted framework-uses-feature)
crate "$ws" framework/stage stage none "paths media-api" \
  'paths = { path = "../paths" }' 'media-api = { path = "../../features/media/api" }'
expect_fail "the framework using a feature's api" W1 "$ws"

ws=$(planted core-uses-other-core)
crate "$ws" features/notifications/core notifications-core none none
crate "$ws" features/media/core media-core none "media-api notifications-core" \
  'media-api = { path = "../api" }' 'notifications-core = { path = "../../notifications/core" }'
expect_fail "a core using another feature's core" F4 "$ws"

ws=$(planted core-tests-use-other-core)
crate "$ws" features/notifications/core notifications-core none none
crate "$ws" features/media/core media-core none "media-api notifications-api" \
  'media-api = { path = "../api" }' 'notifications-api = { path = "../../notifications/api" }' \
  '[dev-dependencies]' 'notifications-core = { path = "../../notifications/core" }'
expect_fail "a core's tests using another feature's core" T1 "$ws"

ws=$(planted core-uses-framework)
crate "$ws" features/media/core media-core none "stage media-api" \
  'media-api = { path = "../api" }' 'stage = { path = "../../../framework/stage" }'
expect_fail "a core using a framework crate not on its list" F2 "$ws"

ws=$(planted front-uses-core)
crate "$ws" fronts/cli cli none media-core 'media-core = { path = "../../features/media/core" }'
expect_fail "a front using a feature's core" W2 "$ws"

ws=$(planted front-uses-app)
crate "$ws" fronts/cli cli none app 'app = { path = "../../app" }'
expect_fail "a front using the app" W2 "$ws"

ws=$(planted not-allowed)
crate "$ws" features/media/api media-api none none \
  'notifications-api = { path = "../../notifications/api" }'
expect_fail "a dependency missing from the allowed list" D2 "$ws"

ws=$(planted no-header)
sed -i '/^# Allowed outside crates/d' "$ws/framework/paths/Cargo.toml"
expect_fail "a Cargo.toml without its allowed lists" D1 "$ws"

ws=$(planted list-on-header)
sed -i 's/^# Allowed outside crates: none/# Allowed outside crates: serde/' "$ws/framework/paths/Cargo.toml"
expect_fail "an allowed list written on its header line" D1 "$ws"

ws=$(planted list-line-malformed)
crate "$ws" features/media/api media-api none notifications-api \
  'notifications-api = { path = "../../notifications/api" }'
sed -i 's/^# - notifications-api/#  - notifications-api/' "$ws/features/media/api/Cargo.toml"
expect_fail "a malformed line in an allowed list" D1 "$ws"

ws=$(planted lists-and-layout)
crate "$ws" framework/stage stage none "paths media-api" \
  'paths = { path = "../paths" }' 'media-api = { path = "../../features/media/api" }'
sed -i '/^# Allowed outside crates/d' "$ws/framework/stage/Cargo.toml"
expect_fail "a broken allowed list hiding a layout break" W1 "$ws"

ws=$(planted nested-framework)
sed -i 's|^members = \[|members = ["framework/stage/extra", |' "$ws/Cargo.toml"
crate "$ws" framework/stage/extra extra none none
expect_fail "a framework crate nested a level too deep" C1 "$ws"

ws=$(planted outside-layout)
sed -i 's|^members = \[|members = ["tools/*", |' "$ws/Cargo.toml"
crate "$ws" tools/thing thing none none
expect_fail "a crate outside the layout folders" C1 "$ws"

ws=$(planted not-member)
crate "$ws" features/notifications/ui notifications-ui none none
expect_fail "a crate in a layout folder but in no workspace's members" C2 "$ws"

ws=$(planted ui-in-the-daemon-workspace)
sed -i 's|^members = \["app", |members = ["features/notifications/ui", "app", |' "$ws/Cargo.toml"
crate "$ws" features/notifications/ui notifications-ui none none
expect_fail "a ui crate in the daemon side's workspace" C2 "$ws"

# A daemon-side crate using a UI crate (rule U1) never gets this far: cargo
# takes a path dependency under the root for a member of the root's
# workspace and refuses the one that names app-ui's.
ws=$(planted core-uses-gpui)
crate "$ws" features/media/core media-core gpui "media-api notifications-api" 'gpui = "1"' \
  'media-api = { path = "../api" }' 'notifications-api = { path = "../../notifications/api" }'
expect_fail "a core using GPUI" U1 "$ws"

ws=$(planted ui-uses-core)
ui_crate "$ws" features/media/ui media-ui none "media-api look media-core" \
  'media-api = { path = "../api" }' 'look = { path = "../../../framework-ui/look" }' \
  'media-core = { path = "../core" }'
expect_fail "a ui using its feature's core" F3 "$ws"

ws=$(planted ui-uses-framework)
ui_crate "$ws" features/media/ui media-ui none "media-api look paths" \
  'media-api = { path = "../api" }' 'look = { path = "../../../framework-ui/look" }' \
  'paths = { path = "../../../framework/paths" }'
expect_fail "a ui using a framework crate not on its list" F3 "$ws"

ws=$(planted framework-ui-uses-feature)
ui_crate "$ws" framework-ui/look look none "carrier media-api" \
  'carrier = { path = "../../framework/carrier" }' 'media-api = { path = "../../features/media/api" }'
expect_fail "a framework-ui crate naming a feature" U2 "$ws"

ws=$(planted ui-program-uses-core)
sed -i 's|^media-ui = .*|&\nmedia-core = { path = "../features/media/core" }|; s|^# - look$|&\n# - media-core|' \
  "$ws/app-ui/Cargo.toml"
expect_fail "the UI program using a core" U3 "$ws"

ws=$(planted lints-differ)
sed -i 's|^unwrap_used = "deny"|unwrap_used = "warn"|' "$ws/app-ui/Cargo.toml"
expect_fail "the UI side's lints differing from the daemon side's" S1 "$ws"

ws=$(planted publishable)
sed -i '/^publish = false$/d' "$ws/framework/carrier/Cargo.toml"
expect_fail "a crate that can be published" S2 "$ws"

ws=$(planted no-manifest)
rm "$ws/features/media/feature.toml"
expect_fail "a feature crate without its feature.toml" C3 "$ws"

if ((failed)); then
  exit 1
fi
echo "check-layout.sh: all cases pass"
