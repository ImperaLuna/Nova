# Run `just` to list recipes. Everything here works inside `nix develop`.

# Codex model for `just review`
review_model := "gpt-6.1-sol"

default:
    @just --list

# Everything CI runs, except e2e
check: fmt-check lint lint-nix lint-traps deny test lint-qml test-qml test-install test-vm-failures test-stack shaders-check

# What scripts/stack.sh restack runs on each branch of a stack before pushing it:
# `check` without the slow advisory fetch and the script tests
check-branch: fmt-check lint lint-nix test lint-qml test-qml shaders-check

# Rust unit tests
test:
    cargo test --workspace

# The Rust tests in CI's Nix sandbox (no network, no setuid, no host tools):
# run before opening a PR, since some tests only fail there
test-sandbox:
    nix build -L --no-link ".#checks.$(nix eval --impure --raw --expr builtins.currentSystem).tests"

# Test and script patterns that flaked before (scripts/lint-traps.sh)
lint-traps:
    scripts/lint-traps.sh

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

# Rewrite ui/daemon/protocol.js's generated names from crates/nova-protocol
protocol-js:
    NOVA_WRITE_PROTOCOL_JS=1 cargo test -p nova-protocol js::protocol_js_is_current

# Review changed insta snapshots (crates/*/src/snapshots)
snapshots:
    cargo insta review

# qmllint over ui/ (fails on any warning)
lint-qml:
    scripts/qmllint.sh $(echo "$NOVA_QML_IMPORTS" | tr ':' ' ')

# install.sh against a fake release, no network needed
test-install:
    scripts/test-install.sh install.sh

# scripts/vm-failures.sh (CI's VM failure tracker) on recorded CI logs
test-vm-failures:
    scripts/test-vm-failures.sh

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

system := `nix eval --raw --impure --expr builtins.currentSystem 2>/dev/null || true`

# NixOS VM test on real niri or Hyprland: lifecycle and screenshots (needs KVM)
e2e-vm compositor="niri":
    nix build -L --no-link .#checks.{{system}}.e2e-{{compositor}}

# Run the VM test and approve its screenshots as e2e/golden/<compositor>
e2e-approve compositor="niri":
    mkdir -p e2e/golden/{{compositor}}
    cp --no-preserve=mode "$(nix build -L --no-link --print-out-paths .#checks.{{system}}.e2e-{{compositor}}.vm)"/*.png e2e/golden/{{compositor}}/
    git status --short e2e/golden/{{compositor}}

# Nova's qmllint and VM tests on the newest stable Quickshell release (needs KVM)
newest-quickshell:
    scripts/newest-quickshell.sh

# Record what niri or Hyprland sends novad, in a VM, for novad's compositor tests (needs KVM)
record compositor="niri":
    rm -rf crates/novad/src/compositor/recorded/{{compositor}}
    cp -r --no-preserve=mode "$(nix build -L --no-link --print-out-paths .#checks.{{system}}.e2e-{{compositor}}.record)/recorded" crates/novad/src/compositor/recorded/{{compositor}}

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
