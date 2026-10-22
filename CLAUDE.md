# Nova

A desktop shell (bar, launcher, notifications, panels) for Wayland. Niri on
NixOS first, Hyprland and other distros after. Priorities, in order: looks and
feature completeness, then modularity and portability, then performance.

`README.md` is written by hand by the maintainer. Don't edit it.

## Layout

- `crates/nova-protocol`: the JSON-lines wire format between novad and the UI.
  `ui/daemon/protocol.js` mirrors it; change both together and keep the tests
  on both sides matching.
- `crates/nova-features`: loads and validates `features/<id>/feature.toml`.
- `crates/novad`: the daemon. Owns all logic and state (compositor IPC,
  search, clipboard, feature status). Listens on
  `$XDG_RUNTIME_DIR/nova/novad.sock`.
- `crates/nova-cli`: the `nova` binary (`features`, `ping`; `setup`, `doctor`
  and `ipc` later).
- `features/`: one folder per optional feature. The manifest is the single
  source of truth for its packages per distro and GPU. Nix, `nova setup` and
  novad all read it. A manifest with `daemon = "x"` needs a matching cargo
  feature `x` in `crates/novad/Cargo.toml` (a test enforces this).
- `ui/`: the Quickshell (QML) UI. `shell.qml` is the entry point.
  - `theme/Tokens.qml`: design tokens. Components never hard-code colors,
    radii, sizes or durations.
  - `daemon/Daemon.qml`: the only socket to novad. Everything else reads its
    properties or calls `Daemon.call(cmd, args, callback)`.
  - `tests/`: QtTest files (`tst_*.qml`), run headless.
- `e2e/run.sh`: headless sway with two monitors + novad + UI. Fails on QML
  errors or no connection; warns when the UI's Pss is over the memory target.
- `scripts/qmllint.sh`: qmllint over `ui/`, failing on any warning.

## Rules

- Logic goes in novad, the UI only renders. If QML needs more than a few lines
  of JavaScript, it probably belongs in the daemon.
- One socket between novad and the UI, with concerns split in code: each
  service is its own QML singleton (e.g. `Workspaces`, `Media`) built on
  `Daemon.qml`, and its own Rust module in novad. Clients subscribe to the
  event topics they need; don't add a second socket.
- The UI checks capabilities that novad reports, never which compositor is
  running (no `isNiri`).
- A feature that is off or missing dependencies is never loaded by the UI.
- Window model is "stage": one full-screen, masked layer surface per monitor.
  The bar, panels and popups are items on it so they can morph into each
  other. Clicks pass through everywhere outside the mask, so keep the mask
  exactly the size of what's visible. Don't add separate layer windows for UI
  pieces without a reason (the 2px auto-hide reveal strip is one).
- The stage surface never reserves space (`exclusionMode: ExclusionMode.Ignore`),
  so opening the launcher or a panel never moves app windows. Space for a
  visible bar is reserved by a separate, empty exclusion-zone surface whose
  size changes only when the bar itself shows or hides.
- Memory target: 250 MB Pss per monitor (500 MB on two). It's a goal, not a
  hard limit; e2e warns when it's exceeded. Unload what isn't visible (Loader
  `active`), set `sourceSize` on images, don't create per-screen copies of
  things one screen needs.
- Quickshell is pinned to the nixpkgs version (0.3.2). `just docs` clones its
  source into `docs/quickshell` for checking APIs; trust it over memory.

## Security

Security and performance are ongoing work, reviewed at every milestone.

- Anything on the socket is untrusted: any local process can connect.
  Validate every request, reject unknown fields, and cap message sizes.
- Never build shell strings. Launch apps and run package managers with argv
  lists (parse `.desktop` `Exec` keys properly); never `sh -c` with user,
  config or manifest data.
- `nova setup` asks for root only for the package-manager call itself, and
  shows the exact command first.
- The clipboard can hold passwords: skip entries marked as secret by password
  managers, cap history size, keep it in memory or a 0600 file, and make
  clearing easy.
- Network calls (weather, usage) use HTTPS only, and no secrets go in logs.

## Performance

- Event-driven, no polling loops: novad pushes changes, and QML binds to them.
- Measure, don't guess: a change that could affect memory or frame time
  states its before/after Pss (from `just e2e`) in the PR.
- Animations and shaders run only while visible.

## Tests

Prefer integration tests: run the real pieces together (a real novad on a
temp socket, real manifest files, the real QML component) rather than
mocking each dependency. Fake only what can't run in a test: the compositor,
hardware, the network. Fake those with recorded real data (captured niri or
Hyprland event streams, saved HTTP responses), not hand-written mocks. A
test that mocks the thing it's testing proves nothing; if a test needs many
mocks, test one level up instead.

## Commands

Run inside `nix develop`:

- `just check`: formatting (Rust, QML, Nix), clippy, statix/deadnix, Rust
  tests, qmllint, QML tests (what CI runs).
- `just fmt`: format everything.
- `just snapshots`: review changed insta snapshots. The protocol's wire format
  is a snapshot, so a protocol change shows up here.
- `just e2e`: the headless end-to-end run; screenshot in `e2e/out/`.
- `just run`: novad and the UI on your current session.
- `nix flake check -L`: the same checks as CI, sandboxed.

Every change goes through a pull request; `main` is protected. CI runs on a
self-hosted runner on the maintainer's desktop, so PR checks wait while it is
off (GitHub drops jobs queued over 24 hours; re-run them).
