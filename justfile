# Run `just` to list recipes. Everything here works inside `nix develop`.

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
    statix check .
    deadnix --fail .

# Review changed insta snapshots (crates/*/src/snapshots)
snapshots:
    cargo insta review

# qmllint over ui/ (fails on any warning)
lint-qml:
    scripts/qmllint.sh $(echo "$NOVA_QML_IMPORTS" | tr ':' ' ')

# QML unit tests (ui/tests), no display needed
test-qml:
    cd ui && QT_QPA_PLATFORM=offscreen qmltestrunner -input tests

# Headless sway + novad + UI: QML errors, connection, Pss budget, screenshot
e2e: build
    e2e/run.sh target/debug/novad

build:
    cargo build --workspace

# Swap your running bar for this checkout's Nova; Ctrl+C puts it back
run: build
    scripts/run.sh target/debug/novad

# Clone Quickshell's source (v0.3.2) into docs/ for reading its API
docs:
    test -d docs/quickshell || git clone --depth 1 --branch v0.3.2 https://github.com/quickshell-mirror/quickshell docs/quickshell
