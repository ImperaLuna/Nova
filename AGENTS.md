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
  its packages. Its daemon side is a novad module: an `impl Module` in its
  own folder plus one line in the `modules!` registry
  (`app/src/modules/`; the trait and the macro are `nova-modules`); apps come from the shared app index in
  `Context`, never from another module. A feature
  that is off or missing dependencies is never loaded by the UI. Its `nix`
  list is what the flake's modules (`programs.nova`, packaging/nix) install
  when it's on, so never ask users to add a feature's packages by hand.
  A feature with `modes` (and a `default-mode`) gets the picked mode from
  `features.toml`'s `[modes]` or the Nix `mode` option, carried on its
  `FeatureState`; one feature never grows a second switch for the same choice.
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
- Glass thickens over bright windows from a small copy of the screen: one
  `Backdrop` per window (a texture draws only in its own window), which
  every `Glass` in that window finds by itself. Copies are taken only while
  no panel or pop-up is out (`quiet`), so glass never sees itself; a window
  without one keeps the fixed tint.
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

## Layout

A crate's folder decides what it may use.

- `framework/<name>/`: the shared part every feature plugs into. It never
  names a feature. Wire types that aren't a feature's sit in their own
  light `-info` crate (`app-info`, `daemon-info` for novad's own messages,
  `desktop-info`, `host-info`, `theme-info`: plain serde types, with their
  `Event` and `TOPIC` where they have one), so api crates and fronts carry
  them without the code that builds them. An `-info` crate uses only
  `nova-carrier` and other `-info` crates (rule I1).
- `features/<id>/api/`: the feature's own messages and types, what other
  crates may use. `features/<id>/core/`: its work, the part that talks to
  the system and draws nothing. `features/<id>/ui/`: reserved for the GPUI
  ports. They sit beside the feature's `feature.toml`.
- `fronts/<name>/`: other ways in, the clients and outside interfaces
  (`fronts/cli`: the `nova` command's socket commands). The socket server
  itself is part of the framework.
- `app/`: wiring only. It starts everything and lists the features. The
  `nova` binary (`app/src/bin/nova`) parses the command line and keeps the
  commands that run Nova itself (start, restart, quit, setup, doctor,
  config, claude-statusline, daemon).

A glob in the workspace's `members` that matches nothing is an error to
cargo, so the first crate of a kind adds its glob. Each `Cargo.toml` opens
with the outside and workspace crates it may use (`# Allowed outside
crates:`, `# Allowed workspace crates:`, one `# - name` line each, or `none`
on the header line); a new dependency goes on that list, with its reason in
the PR.

`just layout` (`scripts/check-layout.sh`, in `just check` and CI) enforces
these rules and names the one a change breaks:

| Rule | A crate | What to do instead |
|---|---|---|
| C1 | sits in one of the folders above | move it there |
| C2 | in those folders is a workspace member | add its folder or kind's glob to `members` |
| C3 | under `features/<id>/` has a `feature.toml` beside it | name the folder after the feature's id |
| W1 | in `framework/` uses only other framework crates | let the feature plug in (a module, a port, a message type) |
| W2 | in `fronts/` uses only listed framework crates and api crates | send a message |
| I1 | named `framework/<name>-info` uses only `nova-carrier` and other `-info` crates | move the code to the crate that builds the types |
| F1 | `api` uses only listed framework crates and api crates | move the code to `core` |
| F2 | `core` uses only listed framework crates and api crates, never a `ui` | send an event |
| F3 | `ui` uses only listed framework crates and api crates, never a `core` | send a message |
| F4 | in a feature never uses another feature's `core` or `ui`, a front or the app | use its api, or a port in the framework |
| T1 | in a feature has no dev-dependency on another feature's `core` or `ui`, unless `DEV_EXCEPTIONS` lists the pair | use a fake from the framework, or list the pair with its reason |
| D1 | `Cargo.toml` has both allowed lists | add them |
| D2 | depends only on crates its allowed lists name | add it to the list and say why in the PR |

The framework crates each half may use are listed at the top of the
script. A framework crate joins a list only when every crate of that kind
may use it: a daemon-side part never on the `ui` list, a screen-side part
never on the `core` list. Dev-dependencies are checked for T1 only.
Changing a rule or a list is a layout change: say so in the PR.

### Moving a feature out of novad

The steps the novad move's feature tickets follow (IMP-368).
`features/resources` is the worked example: read its two crates and
`app/src/resources.rs` first. A move is not a rewrite: no
behaviour changes, `ui/daemon/protocol.js` stays byte for byte the same,
and every test keeps its expectations (a test that moves keeps its
assertions). The feature is `<id>` (its folder name), its crates
`nova-<id>-api` and `nova-<id>` (`_` for `-` in Rust paths).

1. **The api crate**, `features/<id>/api`: the feature's messages, with
   their doc comments. Its own `Command`
   enum (`#[serde(tag = "cmd", content = "args", rename_all = "snake_case",
   deny_unknown_fields)]`), its own `Event` enum (`tag = "event", content =
   "data"`) implementing `nova_carrier::Published`, its topics as `pub const
   TOPIC: Topic = Topic::named("<wire name>")`, and the types those carry. It uses only
   `nova-carrier`, the light type crates whose types its messages carry
   (`FRAMEWORK_FOR_API`, like `nova-app-info`'s `App`; never the lookup
   crates themselves), and other api
   crates. Done when no other crate defines them.
2. **The core crate**, `features/<id>/core`: novad's module folder (or
   file) moved with `git mv`, `mod.rs` becoming `lib.rs`. `crate::` paths
   to shared helpers become the framework crates that hold them
   (`nova_base::lock`, `nova_hub`, `nova_log`, `nova_process::procfs`, ...).
   All of the feature's work and state live here. What the core may use is
   `FRAMEWORK_FOR_CORE` in `scripts/check-layout.sh`; a shared helper still
   in `app/` (apps, icons, the compositor) waits for its own ticket
   (ask the overseeing thread) rather than being copied. Done when the
   folder in `app/src` is gone and the core builds alone.
3. **The wiring**, `app/src/<id>.rs`, under novad's old `mod <id>;`
   line: a newtype over the core that implements `Module` (its `ID`,
   `Sources`, `Config`, `start` building the core from `Context`, and
   `TOPIC` as the api's constant whenever the feature has a topic, even
   with no state to send on subscribe), and `pub fn answer(daemon, caller,
   command) -> Answer` for the api's `Command`. It only adapts the core to novad's
   `App`, `Context` and `Daemon`, so IMP-381 can turn novad into `app`
   without moving logic again. The registry line in `modules/mod.rs` keeps
   its place.
4. **The shared lists.** These are where parallel moves meet, so each entry
   goes in a fixed place:
   - `Cargo.toml` `[workspace.dependencies]`: the two crates in the feature
     block at its end, sorted by feature id (`nova-<id>-api`, then
     `nova-<id>`). `members` already has the globs.
   - `app/Cargo.toml`: both crates in the allowed list and in
     `[dependencies]`, in alphabetical order.
   - `app/src/requests.rs` `router()`: one
     `.route::<nova_<id>_api::Command>(crate::<id>::answer, |_, _, _| {})`
     after novad's own (`Own`) route, the feature routes sorted by feature id (the
     router lists names in protocol.js's order, so errors don't move). Its
     arms leave `answer` and `follow_up`, and its `fn <id>()` lookup goes.
     A module's topic needs no line in `follow_up`: its catch-all asks the
     modules (`Module::TOPIC`), and `each_topic_has_one_owner` checks it.
   - `app/src/topics.rs` `TOPICS`: the api's constant, at the end for a
     new topic (protocol.js adds a new name at the end of its group).
   - `app/src/protocol_js.rs` `events()`: the api's `Event` after
     novad's own and the `-info` crates', sorted by feature id.
   - `fronts/cli`: the api's `Command` as a variant of `ipc.rs`'s
     `IpcCommand` (with its branch in `parse`), and its `Event` as a variant
     of `client.rs`'s `Sent`, both after novad's own (`Daemon`) and the
     `-info` crates', sorted by feature id; the
     crate on both allowed lists. A CLI command that sent or read the moved
     messages matches the new variant.
   - `app/src/tests/wire.rs`: the wire format snapshot gets the feature's
     lines, built from the api's types; `app/tests/decode_request.rs` gets
     a strategy and round trip for the feature's commands.
   Done when
   `cargo test -p novad requests::tests::every_command_has_one_route`
   passes.
5. **Check.** `just check` (layout, clippy, every test), and
   `git diff main -- ui/daemon/protocol.js` shows nothing: the generator
   keeps each name where the file has it. The wire format snapshot is
   unchanged. Test-only helpers that cross crates go behind a
   `test-support` cargo feature. Done when all of these hold.

## Engineering principles

- One source of truth. A value used in more than one place (a color, size,
  duration, timeout, socket path, command name, event name) is defined once
  and referenced everywhere else: design values in `theme/Tokens.qml`, wire
  names in the api or `-info` crate of their message (the envelope and the
  log feed in `nova-carrier`), Rust constants in the crate
  that owns them. Never copy a literal into a second file. The wire names in
  `ui/daemon/protocol.js` are generated from the types novad serves: change
  the Rust types, then run `just protocol-js`.
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

## Rules and their checks

The rules above that a check enforces, and the check. Nova Files uses the
same lints and limits, so both repos read the same.

| Rule | Enforced by |
|---|---|
| An exception is `#[expect]` with a reason, never `#[allow]` | Clippy `allow_attributes`, `allow_attributes_without_reason` |

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

Run `just` inside `nix develop` to list the recipes. CI runs the flake checks:
what `just check` runs plus the headless e2e on every push, and the VM tests on
niri and Hyprland only once a PR is out of draft (so mark a PR ready, or open
it ready, to get VM results) and has no PR stacked on it. Locally, `just check` and `just e2e` cover most of it; the VM
tests need KVM. Before opening a PR, also run `just test-sandbox`: it runs the
Rust tests in CI's Nix sandbox, where a test that needs the network, setuid
bits or a host tool fails although `cargo test` passes (cloud sessions can't
fetch its GitHub inputs, so there CI is the first sandbox run). Without KVM (cloud sessions), approve goldens with the "Approve
goldens" workflow from main on the PR branch instead of `just e2e-approve`.
`just run` logs to `~/.local/state/nova-dev`.

A failed VM subtest prints who had the keyboard and the UI's and the
compositor's logs (`print_failure_state` in `e2e/vm/lifecycle.py`); read that
before re-running. Each failed subtest is listed in the "VM test failures"
issue (`scripts/vm-failures.sh`), which marks one that failed on more than one
PR as a repeat and says so on the PR: a repeat gets a fix ticket instead of
another re-run.

Every change goes through a pull request; `main` is protected.

A stack of PRs is one chain: the bottom PR targets main, each one above
targets the branch below, and a branch merges only its own base. Its top PR
runs the VM tests for all of it. `scripts/stack.sh status|restack` checks the
chain and merges each base upward, running `just check-branch` on every
branch before pushing it (run it inside `nix develop`); its header says more.
A PR whose base moved on without it gets a red `stack` status: restack it.

A batch of PRs is ready for the maintainer only when `stack.sh status` is clean
for every stack and each branch passed `just check-branch`. While a batch's
merges land one by one, other threads' ready PRs get the `hold` label until
the batch is in. Desktop test passes for a batch start once the CI runners
`nova-1` and `nova-2` are idle.

Everything a session makes for Nova on the desktop stays inside `~/Projects/Nova`;
never create a folder elsewhere in `~`, such as `~/Projects/nova-imp121`, even
when a prompt names one. Scratch output (measurements, logs, screenshots,
scripts, audit data) goes in the git-ignored `~/Projects/Nova/.scratch/<ticket>/`
(for example `.scratch/imp-121/`), in a cloud session's scratchpad, or in a
worktree's ignored `e2e/out/`; never in the repo root. Anything the maintainer
should keep goes in the PR or an artifact, and the session deletes its
`.scratch/<ticket>/` when its work is done. Worktrees on the desktop go in
`~/Projects/Nova/.claude/worktrees/<branch>`. A daily timer removes worktrees whose
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
