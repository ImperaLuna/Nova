# Run `just` to list recipes. Everything here works inside `nix develop`.

default:
    @just --list

# Everything CI runs
check: fmt-check layout test-layout lint lint-nix lint-traps file-length deny test test-install test-stack

# What scripts/stack.sh restack runs on each branch of a stack before pushing it:
# `check` without the slow advisory fetch and the script tests
check-branch: fmt-check layout lint lint-nix file-length test

# The UI side: nova-ui and the crates that use GPUI, in their own cargo
# workspace (AGENTS.md, Layout). Its recipes run inside `nix develop .#ui`.
ui_manifest := "app-ui/Cargo.toml"

# What CI checks on the UI side
ui-check: ui-fmt-check ui-lint ui-test ui-deny

# Build nova-ui into app-ui/target
ui-build:
    cargo build --manifest-path {{ui_manifest}} --workspace

ui-fmt:
    cargo fmt --manifest-path {{ui_manifest}} --all

ui-fmt-check:
    cargo fmt --manifest-path {{ui_manifest}} --all --check

# Clippy on the UI side, with warnings as errors
ui-lint:
    cargo clippy --manifest-path {{ui_manifest}} --workspace --all-targets -- -D warnings

# The UI side's unit tests (pure logic only: it has no UI test suite)
ui-test:
    cargo test --manifest-path {{ui_manifest}} --workspace

# Advisories, licenses and sources of what GPUI brings in (app-ui/deny.toml), latest advisories
ui-deny:
    cargo deny --manifest-path {{ui_manifest}} check

# Rust unit tests
test:
    cargo test --workspace

# The Rust tests in CI's Nix sandbox (no network, no setuid, no host tools):
# run before opening a PR, since some tests only fail there
test-sandbox:
    nix build -L --no-link ".#checks.$(nix eval --impure --raw --expr builtins.currentSystem).tests"

# Crates sit in the layout and use only what their folder and allowed lists permit (AGENTS.md, Layout)
layout:
    scripts/check-layout.sh

# scripts/check-layout.sh on throwaway workspaces with planted mistakes
test-layout:
    scripts/test-check-layout.sh

# Test and script patterns that flaked before (scripts/lint-traps.sh)
lint-traps:
    scripts/lint-traps.sh

# No Rust file in a crate over 800 lines (scripts/check-file-length.sh)
file-length:
    scripts/check-file-length.sh

# Comments outside the keep list in AGENTS.md; `--report` only counts them.
comments *flags:
    scripts/check-comments.sh {{ flags }}

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Advisories, licenses, duplicate crates and sources (deny.toml), latest advisories
deny:
    cargo deny check

# Format Rust and Nix
fmt:
    cargo fmt --all
    nixfmt $(git ls-files '*.nix')

fmt-check:
    cargo fmt --all --check
    nixfmt --check $(git ls-files '*.nix')

# statix and deadnix over the Nix files
lint-nix:
    statix check --ignore 'docs/*' .
    deadnix --fail $(git ls-files '*.nix')

# Review changed insta snapshots (each crate's snapshots folders)
snapshots:
    cargo insta review

# install.sh against a fake release, no network needed
test-install:
    scripts/test-install.sh install.sh

# scripts/stack.sh (the stack keeper's tool) on a throwaway stack
test-stack:
    scripts/test-stack.sh

build:
    cargo build --workspace

# Swap your running bar for this checkout's Nova; Ctrl+C puts it back
run: build
    scripts/run.sh target/debug/nova
