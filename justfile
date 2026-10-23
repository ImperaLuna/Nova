# Run `just` to list recipes. Everything here works inside `nix develop`.

# Codex model for `just review`
review_model := "gpt-6.1-sol"

default:
    @just --list

# Everything CI runs, except e2e
check: fmt-check lint lint-nix test lint-qml test-qml

# Rust unit tests
test:
    cargo test --workspace

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

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

# Review changed insta snapshots (crates/*/src/snapshots)
snapshots:
    cargo insta review

# qmllint over ui/ (fails on any warning)
lint-qml:
    scripts/qmllint.sh $(echo "$NOVA_QML_IMPORTS" | tr ':' ' ')

# QML unit tests (ui/tests), no display needed
test-qml:
    cd ui && QT_QPA_PLATFORM=offscreen qmltestrunner -input tests

# Headless sway + Nova's daemon + UI: QML errors, connection, Pss budget, screenshot
e2e: build
    e2e/run.sh target/debug/nova

build:
    cargo build --workspace

# Swap your running bar for this checkout's Nova; Ctrl+C puts it back
run: build
    scripts/run.sh target/debug/nova

# Clone the source of the dev shell's Quickshell (same version) into docs/ for reading its API
docs:
    test -d docs/quickshell || git clone --depth 1 --branch "v$(quickshell --version | cut -d' ' -f2)" https://github.com/quickshell-mirror/quickshell docs/quickshell

# Needs the Codex CLI and `codex login`; not part of the dev shell or CI.
# Codex reviews this branch against `base` (sends the diff to OpenAI)
review base="main":
    codex review --base {{base}} -c model="{{review_model}}"
