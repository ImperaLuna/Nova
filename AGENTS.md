# Nova

A desktop shell (bar, launcher, notifications, panels) for Wayland. Niri on
NixOS first, Hyprland and other distros after. Priorities, in order: looks and
feature completeness, then modularity and portability, then performance.

`README.md` is written by hand by the maintainer. Don't edit it.

This file is the only agent instruction file (Claude Code and Codex both read
it). Don't add a `CLAUDE.md` or `CLAUDE.local.md`: Claude Code then skips
`AGENTS.md`.

## Rules

- Logic goes in novad, the UI only renders. If QML needs more than a few lines
  of JavaScript, it probably belongs in the daemon.
- One socket between novad and the UI, with concerns split in code: each
  service is its own QML singleton built on `Daemon.qml` and its own Rust
  module in novad. Clients subscribe to the topics they need.
  Exception: audio (outputs, inputs, app streams, their volume and mute)
  goes through Quickshell's PipeWire service in `ui/volume/service/`, since
  a Rust PipeWire library would break the static musl build and Quickshell
  already keeps the connection. Keep its logic in plain JS (`volume.js`,
  `quick_settings.js`) so ui/tests can check it.
- The UI checks capabilities that novad reports, never which compositor is
  running (no `isNiri`).
- A feature is `features/<id>/feature.toml`, the single source of truth for
  its packages; `daemon = "x"` needs a cargo feature `x` in novad. A feature
  that is off or missing dependencies is never loaded by the UI. Its `nix`
  list is what the flake's modules (`programs.nova`, packaging/nix) install
  when it's on, so never ask users to add a feature's packages by hand.
- Window model is "stage": one masked layer surface per monitor, across the
  top and only as tall as the tallest panel open (niri blurs the whole
  surface behind glass each frame, so its size is the blur cost). The bar,
  panels and popups are items on it so they can morph into each other. Clicks pass through everywhere outside the mask, so keep the mask
  exactly the size and shape of what's visible (rounded corners too, from the
  same radius property the drawing uses). Don't add separate layer windows
  without a reason (the 2px auto-hide reveal strip is one).
- A panel that draws outside the top strip sets `StagePanel.placement`
  ("right" or "center") and its `area` (where it draws while open or
  moving, screen coordinates). It's then drawn on that placement's
  PanelSurface, mapped only while one of its panels is shown and sized to
  their areas; the stage contract (mask, blur, keyboard, dismiss, toggle)
  is the same. One growing out of a stage pill sets `from` to that pill.
- The stage never reserves space, so opening a panel never moves app
  windows. A separate, empty exclusion-zone surface reserves the bar's space
  and changes only when the bar shows or hides.
- Memory target: 250 MB Pss per monitor, a goal rather than a hard limit.
  Unload what isn't visible (Loader `active`), set `sourceSize` on images,
  don't make per-screen copies of what one screen needs.
- `packaging/quickshell.toml` is the tested Quickshell range (no pin, no
  fork); raise `max` only after testing the newer release (the "Newest
  Quickshell" workflow, `just newest-quickshell`). Check Quickshell
  APIs in `docs/quickshell` (`just docs`), not from memory.

## Engineering principles

- One source of truth. A value used in more than one place (a color, size,
  duration, timeout, socket path, command name, event name) is defined once
  and referenced everywhere else: design values in `theme/Tokens.qml`, wire
  names in `nova-protocol` and `ui/daemon/protocol.js`, Rust constants in the
  crate that owns them. Never copy a literal into a second file.
  `ui/daemon/protocol.js` mirrors `nova-protocol`: change both together,
  with matching tests on both sides.
- Each crate, module and QML file opens with a comment saying what it does
  and how. Read it before changing that code and keep it true in the same
  change.
- No magic numbers or strings. Give values a named constant or property that
  says what they mean (`Tokens.radiusRow`, not `14`). Every design value
  (gradient stops, opacities and easing too) lives in `Tokens.qml`, even when
  one component uses it; a component's own properties only alias tokens or
  derive ratios from them.
- Don't repeat logic. When the same JavaScript or Rust shows up twice, move it
  into a shared component, singleton or function before adding a third copy.
- Names say what something is: no `tmp`, `data2` or single letters outside
  short loops.
- Small, focused files and components: each does one job.
- Handle errors where they happen and show them in a useful form. No silent
  `catch {}` or `unwrap()` on input that can fail (tests excepted).
- Design for the general case, then fix root causes. Edge cases (a second
  monitor, a missing feature, a compositor without a capability, an empty
  list) should work because the design covers them, not through special-case
  branches. If a workaround is truly unavoidable (an upstream bug), keep it
  in one place with a comment linking the issue and how to remove it.
- Leave code cleaner than you found it, but keep refactors in their own commit.

## State, threads and files

- A state change and what follows from it (saving it to disk, the snapshot
  last broadcast) happen in one critical section, in order. Building a reply
  for one client never changes shared bookkeeping.
- A worker that can be stopped checks for the stop before each side effect
  (a request, a write, a spawn), not only after it wakes.
- Every wait on another process (connect, read, write, HTTP, a child) has a
  timeout. Code that runs inside another program, like `nova
  claude-statusline`, never waits on novad longer than that.
- When several sources report the same value, the newest by its own
  timestamp wins, not the newest file or the last to arrive.
- File watching: install the watches, then read, so nothing written in
  between is missed. Watch what may not exist yet or may be replaced (a
  folder, a symlink, a home-manager link) through its parent and re-attach
  when that entry changes; watch a symlink's target too. A queue overflow
  (`IN_Q_OVERFLOW`) means lost events: rescan everything.
- Save files by writing a temp file in the same folder and renaming it over
  the old one. Create private files with their mode (`0600`) rather than
  changing it afterwards.

## Security

- Anything on the socket is untrusted. Validate every request, reject
  unknown fields at every level (`deny_unknown_fields` on each struct and
  enum Nova's wire format or config files use), and cap message sizes.
  Compositor IPC is the exception: ignore what's unknown there, so newer
  releases keep working.
- Never build shell strings. Launch apps and run package managers with argv
  lists (parse `.desktop` `Exec` keys properly); never `sh -c` with user,
  config or manifest data.
- `nova setup` asks for root only for the package-manager call itself, and
  shows the exact command first.
- The clipboard can hold passwords: skip entries password managers mark as
  secret, cap history size, keep it in memory or a 0600 file, and make
  clearing easy.
- Network calls use HTTPS only, have a timeout and a size cap on the answer,
  and back off on errors.
- Secrets (tokens, passwords) live in types that don't derive `Debug`,
  `Display` or `Serialize`, so they can't reach a log, an event or a saved
  file by accident. Check a credentials file's owner and mode on the open
  handle, not the path.
- Child processes get the environment they need (never novad's secrets),
  are always waited on, and have their output pipes drained.
- Paths that come from outside (the socket, manifests, `.desktop` files)
  must stay inside the folder they're meant for: no `..`, no absolute paths.
- Text from outside Nova (window titles, notifications, media info) renders
  with `textFormat: Text.PlainText`; any markup is sanitized in novad first.

## Performance

- Event-driven, no polling loops: novad pushes changes, and QML binds to them.
- Measure, don't guess: a change that could affect memory or frame time
  states its before/after Pss (from `just e2e`) in the PR, and one that
  could affect animation smoothness its dropped frames from `just frames`
  on real hardware.
- Animations and shaders run only while visible: `running` checks the
  item's own `visible` and `opacity`, not just the state that started it.

## Tests

Mostly component tests (one component through its real interface, with its
real internals) and integration tests (several together, like the CLI
against a running novad). Unit tests only for small, tricky pure logic;
e2e is the final check. Fake only the compositor, hardware and network,
with recorded real data, not hand-written mocks. QML tests can't load
Quickshell's own types; e2e covers those.

Tests start from what a new user has (folders missing, no saved state)
unless they're about something else. A bug fix comes with a test that fails
without it.

Tests never touch the desktop's session bus, where a test's novad would
queue for the running shell's names: `.cargo/config.toml` sets
`DBUS_SESSION_BUS_ADDRESS=disabled:` for everything cargo runs, and a test
that needs a bus starts its own (`session_bus::TestBus`).

## Commands

Run `just` inside `nix develop` to list the recipes. CI runs `nix flake
check`: what `just check` runs, plus the headless e2e and the VM tests on niri
and Hyprland. Locally, `just check` and `just e2e` cover most of it; the VM
tests need KVM. Without KVM (cloud sessions), approve goldens with the "Approve
goldens" workflow from main on the PR branch instead of `just e2e-approve`.
`just run` logs to `~/.local/state/nova-dev`.

Every change goes through a pull request; `main` is protected.

Everything a session makes for Nova on the desktop stays inside `~/Projects/Nova`;
never create a folder elsewhere in `~`, such as `~/Projects/nova-imp121`, even
when a prompt names one. Scratch output (measurements, logs, screenshots,
scripts, audit data) goes in the git-ignored `~/Projects/Nova/.scratch/<ticket>/`
(for example `.scratch/imp-121/`), in a cloud session's scratchpad, or in a
worktree's ignored `e2e/out/`; never in the repo root. Anything the maintainer
should keep goes in the PR or an artifact, and the session deletes its
`.scratch/<ticket>/` when its work is done. Worktrees on the desktop go in
`~/Projects/Nova/.worktrees/<branch>`. A daily timer removes worktrees whose
PR is merged or closed, and scratch nobody touched for a week.

On the desktop, use `scripts/desktop/` rather than doing these steps by hand
(each script's header says more): `worktree <branch>` fetches a branch into
its worktree (over HTTPS, so no SSH workaround), `isolated-novad <branch>
<feature,...> [topic...]` runs that branch's novad with only those features
beside the live Nova and prints the topics' events, and `apply-nix-config`
applies `~/nix-config` once the CI runners are idle, rolling back on failure. Never restart or stop
`nova.service` or the live novad unless asked.

Test runs on the desktop stay off the maintainer's screen. Frame captures and
screenshots run headless (`just e2e`, or `e2e/run.sh`'s sway with grim on its
output), and niri- or Hyprland-only behaviour goes to the VM tests; never open
a nested compositor window, which lands on the maintainer's workspace (on
Hyprland it takes the screen). `just run` and `just frames` swap the real bar,
so run them only when asked.

## Review guidelines

For Codex reviews of pull requests.

- Check the rules above, and look hardest for races, lost updates, missed
  file changes, waits without a timeout and secrets that could reach a log.
- Each finding names the steps that trigger it, and says when they're rare.
- Don't flag setups a supported Wayland session can't have (no
  `~/.config`, no `$XDG_RUNTIME_DIR`), what `just check` already enforces,
  or ratios derived from tokens.
