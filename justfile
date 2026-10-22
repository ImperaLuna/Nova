# Run `just` to list recipes. Everything here works inside `nix develop`.

default:
    @just --list

# Everything CI runs, except e2e
check: fmt-check lint test lint-qml test-qml

# Rust unit tests
test:
    cargo test --workspace

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

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

# Run novad and the UI on your current session
run: build
    target/debug/novad & qs -p ui; kill %1

# Clone Quickshell's source (v0.3.2) into docs/ for reading its API
docs:
    test -d docs/quickshell || git clone --depth 1 --branch v0.3.2 https://github.com/quickshell-mirror/quickshell docs/quickshell
