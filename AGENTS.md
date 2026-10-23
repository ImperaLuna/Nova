# Nova

A desktop shell (bar, launcher, notifications, panels) for Wayland. Niri on
NixOS first, Hyprland and other distros after. Priorities, in order: looks and
feature completeness, then modularity and portability, then performance.

`README.md` is written by hand by the maintainer. Don't edit it.

This file is the only agent instruction file (Claude Code and Codex both read
it). Don't add a `CLAUDE.md` or `CLAUDE.local.md`: Claude Code then skips
`AGENTS.md`.

## Layout

- `crates/nova-protocol`: the JSON-lines wire format between novad and its
  clients (the UI and the CLI), plus the log line format.
  `ui/daemon/protocol.js` mirrors it; change both together and keep the tests
  on both sides matching. Clients get `hello`, then only the topics they
  `subscribe` to.
- `crates/nova-paths`: where things live (socket, config, state and log
  files, installed data). novad and the CLI both resolve paths here.
- `crates/nova-quickshell`: the Quickshell versions Nova supports
  (`packaging/quickshell.toml`, compiled in) and the check against them.
- `crates/nova-features`: loads and validates `features/<id>/feature.toml`.
- `crates/novad`: the daemon, a library the `nova` binary runs as
  `nova daemon` (one binary to install; it shows as `Nova` in ps and btop).
  Owns all logic and state (compositor IPC, search, clipboard, feature
  status). Listens on
  `$XDG_RUNTIME_DIR/nova/novad.sock`, runs the Quickshell UI as its child
  (restarting it when it crashes) and writes the log for both:
  `~/.local/state/nova/nova.log`, rotated per novad start, with crash
  snapshots in `crashes/`. CLI tests run a real novad in-process.
- `crates/nova-cli`: the `nova` binary. `nova` starts Nova (or shows its
  status), plus `restart`, `quit`, `status`, `logs`, `version`, and the
  hidden `daemon`. It talks to novad and to the `nova.service` systemd user
  unit; no logic of its own.
  `nova help` also lists planned commands, marked `[planned]`.
- `themes/<id>.toml`: predefined color themes (no wallpaper-generated
  colors). Each has a `[dark]` table and may have `[light]`, setting every
  token of `Palette` in `nova-protocol` exactly once. novad loads the one
  `~/.config/nova/config.toml` names (`[theme] name`, `mode`), falls back to
  Carbonfox dark (compiled in) on any problem, and pushes it on the `theme`
  topic. Tests check every theme for missing or extra tokens and for
  contrast (fg on bg, onAccent on primary, at least 4.5:1).
- `packaging/systemd/nova.service.in`: the user unit; the Nix package fills
  in its paths. Without systemd, `nova` runs `nova daemon` directly.
- `packaging/nix/nova-ui.nix`: the `nova-ui` launcher, Quickshell under the
  name NovaUI so ps and btop show the UI as Nova's. The Nix package compiles
  its path into novad (`NOVA_UI_LAUNCHER`); otherwise novad uses it when it's
  on PATH (the dev shell has it), else plain `quickshell`.
- `features/`: one folder per optional feature. The manifest is the single
  source of truth for its packages per distro and GPU. Nix, `nova setup` and
  novad all read it. A manifest with `daemon = "x"` needs a matching cargo
  feature `x` in `crates/novad/Cargo.toml` (a test enforces this).
- `ui/`: the Quickshell (QML) UI. `shell.qml` is the entry point.
  - `theme/Tokens.qml`: design tokens. Components never hard-code colors,
    radii, sizes or durations. Colors are the theme's, set by `Daemon.qml`
    from the `theme` event; the shell draws nothing until it arrives.
  - `daemon/Daemon.qml`: the only socket to novad. Everything else reads its
    properties or calls `Daemon.call(cmd, args, callback)`.
  - `tests/`: QtTest files (`tst_*.qml`), run headless.
- `e2e/run.sh`: headless sway with two monitors + novad, which runs the UI.
  Fails on QML errors, a crashed UI or no connection; warns when the UI's Pss
  is over the memory target.
- `e2e/vm/`: NixOS VM tests on real niri and Hyprland (flake checks
  `e2e-niri`, `e2e-hyprland`; they need KVM, so run them as Nix builds). One
  VM config and one test script (`lifecycle.py`) shared by both; each
  compositor only says how it starts and quits. They cover what needs a
  systemd user session (starting through `nova.service`, crash restarts,
  `nova restart`/`quit`, novad exiting with the compositor) and screenshot
  each UI state. `compare.sh` diffs the screenshots against
  `e2e/golden/<compositor>/<state>.png` with odiff.
- `scripts/run.sh` (`just run`): swaps your running bar for this checkout's
  Nova and puts the old one back on Ctrl+C or when the terminal closes.
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
- Quickshell: Nova supports a tested range of upstream releases, declared
  once in `packaging/quickshell.toml` (no exact pin across distros, no fork).
  The flake uses the nixpkgs version flake.lock pins, and the
  `quickshell-version` flake check fails when that leaves the range. novad
  warns at start, and `nova status` shows a warning row, when the Quickshell
  it runs is outside it (`crates/nova-quickshell`). Raise `max` only after
  testing Nova on the newer release. `just docs` clones the dev shell's
  Quickshell source into `docs/quickshell` for checking APIs; trust it over
  memory.

## Engineering principles

- One source of truth. A value used in more than one place (a color, size,
  duration, timeout, socket path, command name, event name) is defined once
  and referenced everywhere else: design values in `theme/Tokens.qml`, wire
  names in `nova-protocol` and `ui/daemon/protocol.js`, Rust constants in the
  crate that owns them. Never copy a literal into a second file.
- No magic numbers or strings. Give values a named constant or property that
  says what they mean (`Tokens.radiusPill`, not `14`).
- Don't repeat logic. When the same JavaScript or Rust shows up twice, move it
  into a shared component, singleton or function before adding a third copy.
- Names say what something is. Descriptive variable, property and file names;
  no `tmp`, `data2` or single letters outside short loops.
- Small, focused files and components. Each QML component or Rust module does
  one job; split one that grows past that.
- Handle errors where they happen and show them in a useful form. No silent
  `catch {}` or `unwrap()` on input that can fail (tests excepted).
- Design for the general case, then fix root causes. Edge cases (a second
  monitor, a missing feature, a compositor without a capability, an empty
  list) should work because the design covers them, not through special-case
  branches or workarounds added later. When something breaks in one case, fix
  why it breaks; if a workaround is truly unavoidable (an upstream bug), keep
  it in one place with a comment linking the issue and how to remove it.
- Leave code cleaner than you found it, but keep refactors in their own commit.

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

Most tests are **component** and **integration** tests. Unit and smoke tests
are the minority.

- **Component tests (the main focus):** one component through its real public
  interface, with its real internals. For example, a novad service driven
  through its socket commands, a feature loaded from real manifest files, or
  a QML panel loaded with its real child components.
- **Integration tests (the main focus):** several components working together,
  such as novad and the UI talking over the real socket, or the CLI against a
  running novad.
- **Unit tests:** only for small pure logic that's tricky on its own (parsers,
  version ranges, contrast math).
- **Smoke/e2e** (`e2e/run.sh`, the VM tests in `e2e/vm`): does the whole shell start
  and behave. They're a final check, not the main suite.

Fake only what can't run in a test: the compositor, hardware, the network.
Fake those with recorded real data (captured niri or Hyprland event streams,
saved HTTP responses), not hand-written mocks. If a test needs many mocks,
test one level up instead.

## Commands

Run inside `nix develop`:

- `just check`: formatting (Rust, QML, Nix), clippy, statix/deadnix, Rust
  tests, qmllint, QML tests (what CI runs).
- `just fmt`: format everything.
- `just snapshots`: review changed insta snapshots. The protocol's wire format
  is a snapshot, so a protocol change shows up here.
- `just e2e`: the headless end-to-end run; screenshot in `e2e/out/`.
- `just e2e-vm [niri|hyprland]`: one VM test (needs KVM). `nix flake check`
  runs both.
- `just e2e-approve [niri|hyprland]`: runs a VM test and copies its
  screenshots into `e2e/golden/<compositor>`. Run it after an intended visual
  change and commit the new goldens with it.
- `just run`: this checkout's Nova on your current session, in place of
  your bar until Ctrl+C. Its log goes to `~/.local/state/nova-dev`.
- `nix flake check -L`: the same checks as CI, sandboxed.
- `just review [base]`: a Codex review of the branch against `base` (default
  `main`). Needs the Codex CLI and `codex login` outside the dev shell.

Test every change on the maintainer's desktop (NixOS + niri) before calling it
done, whenever a session there is connected (a Remote Control session or a
connected folder): check the branch out in your own worktree, run `just
check`, then try the change itself (`just run`, the `nova` command it
touches, btop for process names). Say in the PR what was tried there. Don't
commit or push from the desktop; changes go through the branch. With no
connection, say the change is untested on real hardware.

Several sessions can test on the desktop at once, so they mustn't race:

- Never switch branches in `~/Projects/Nova`; the maintainer and other
  sessions use it. Test in a worktree of your own (its own `target/` and
  `e2e/out/`): `git -C ~/Projects/Nova fetch origin <branch>`, then
  `git -C ~/Projects/Nova worktree add ~/Projects/Nova-worktrees/<branch>
  origin/<branch>`. Remove it with `git worktree remove` when the PR is done.
- Threads can end before their PR does, so clean up before you add yours:
  remove every worktree in `~/Projects/Nova-worktrees` whose PR is merged or
  closed, then `git -C ~/Projects/Nova worktree prune`. Each one holds about
  1 GB of build output, and its `result` links keep Nix builds from being
  garbage-collected.
- `just check`, `just e2e` and `nix flake check` are safe in parallel:
  `just e2e` has its own runtime dir, socket and headless sway.
- `just run` swaps out the real bar, so only one runs at a time. A second one
  exits with "another `just run` is using the bar": wait and retry, don't kill
  the other one. Don't touch the live bar, `nova.service` or
  `$XDG_RUNTIME_DIR/nova` any other way.
- Leave uncommitted work in any checkout you didn't create alone (stash it
  with a named message if you must, never discard it).
- CI and the other sessions share the desktop's CPU, so builds can be slow.
  Rerun a timeout before calling it a failure.
- CI jobs can't open `/dev/kvm`; VM tests get KVM only as Nix builds through
  the daemon (flake checks), not as a driver started in a job step.

Every change goes through a pull request; `main` is protected. CI runs on a
self-hosted runner on the maintainer's desktop, so PR checks wait while it is
off (GitHub drops jobs queued over 24 hours; re-run them).

Tickets live in Linear (team IMP). PR titles are `[<ticket>] <type>:
<description>`, where the type is the ticket's Linear label (Feature, Bug,
Improvement), like `[IMP-8] Feature: nova CLI`. The body's first line is
`Part of IMP-<n>` while the ticket has more PRs to come, or `Closes IMP-<n>`
on the PR that finishes it, so Linear moves the ticket to Done only then.

Open PRs as drafts and mark them ready for review only when the work is done.
Codex reviews a PR automatically once, when it becomes ready, so pushes to a
draft don't each trigger a review. For another review after fixes, comment
`@codex review` on the PR.

Review findings (Codex, or any other reviewer bot) are claims to check, not
orders. Before fixing one, reproduce it or trace a real path from a real
caller or input to the failure, then weigh the code the fix adds against
what it prevents. Fix it when the path is real and the fix is small and
proportionate. Otherwise reply on the thread with what you traced and why
it isn't worth the code, and leave the call to the maintainer. Don't add
layers, options or special cases to satisfy a reviewer.
