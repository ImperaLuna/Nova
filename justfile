# Run `just` to list recipes. Everything here works inside `nix develop`.

# Codex model for `just review`
review_model := "gpt-6.1-sol"

default:
    @just --list

# Everything CI runs, except e2e
check: fmt-check layout test-layout lint lint-nix lint-traps file-length deny test lint-qml test-qml test-install test-stack shaders-check

# What scripts/stack.sh restack runs on each branch of a stack before pushing it:
# `check` without the slow advisory fetch and the script tests
check-branch: fmt-check layout lint lint-nix file-length test lint-qml test-qml shaders-check

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

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Advisories, licenses, duplicate crates and sources (deny.toml), latest advisories
deny:
    cargo deny check

# Format Rust, QML and Nix
fmt:
    cargo fmt --all
    scripts/qmlformat.sh
    nixfmt $(git ls-files '*.nix')

fmt-check:
    cargo fmt --all --check
    scripts/qmlformat.sh --check
    nixfmt --check $(git ls-files '*.nix')

# statix and deadnix over the Nix files
lint-nix:
    statix check --ignore 'docs/*' .
    deadnix --fail $(git ls-files '*.nix')

# Rewrite ui/daemon/protocol.js's generated names from the types novad serves
protocol-js:
    NOVA_WRITE_PROTOCOL_JS=1 cargo test -p novad protocol_js::protocol_js_is_current

# Review changed insta snapshots (each crate's snapshots folders)
snapshots:
    cargo insta review

# qmllint over ui/ (fails on any warning)
lint-qml:
    scripts/qmllint.sh $(echo "$NOVA_QML_IMPORTS" | tr ':' ' ')

# install.sh against a fake release, no network needed
test-install:
    scripts/test-install.sh install.sh

# scripts/stack.sh (the stack keeper's tool) on a throwaway stack
test-stack:
    scripts/test-stack.sh

# Compile ui/**/*.frag into the .qsb files QML loads (committed)
shaders:
    scripts/shaders.sh

# Fail if a committed .qsb doesn't match its .frag
shaders-check:
    scripts/shaders.sh --check

# QML unit tests (ui/tests), no display needed
test-qml:
    cd ui && QT_QPA_PLATFORM=offscreen qmltestrunner -input tests

# Headless sway + Nova's daemon + UI: QML errors, connection, Pss budget, screenshot
e2e: build
    e2e/run.sh target/debug/nova

# Nova's build and qmllint on the newest stable Quickshell release
newest-quickshell:
    scripts/newest-quickshell.sh

build:
    cargo build --workspace

# Swap your running bar for this checkout's Nova; Ctrl+C puts it back
run: build
    scripts/run.sh target/debug/nova

# Open and close a panel on this desktop and report the vblanks it misses (swaps the bar like `just run`)
frames times="20" panel="ai-usage": build
    scripts/frames.sh target/debug/nova {{times}} {{panel}}

# Clone the source of the dev shell's Quickshell (same version) into docs/ for reading its API
docs:
    test -d docs/quickshell || git clone --depth 1 --branch "v$(quickshell --version | cut -d' ' -f2)" https://github.com/quickshell-mirror/quickshell docs/quickshell

# Needs the Codex CLI and `codex login`; not part of the dev shell or CI.
# Codex reviews this branch against `base` (sends the diff to OpenAI). Fetches
# main first, so the diff isn't taken against a stale local main.
review base="origin/main":
    git fetch -q origin main
    codex review --base {{base}} -c model="{{review_model}}"
