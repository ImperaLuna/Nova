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
  Exception: the Quickshell UI reads audio (outputs, inputs, app streams,
  their volume and mute) through Quickshell's PipeWire service in
  `ui/volume/service/`. Keep its logic in plain JS (`volume.js`,
  `quick_settings.js`) so ui/tests can check it. The GPUI ports read the
  `sound` topic instead: novad's sound core (`features/sound/core`) follows
  PipeWire through its PulseAudio socket with a pure Rust client, since a
  library that links PipeWire would break the static musl build.
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

- The UI is moving from Quickshell to GPUI (IMP-368). Until the switch
  both are on main: the QML shell under `ui/`, which stays the default and
  must keep working, and `nova-ui`, the GPUI program (`app-ui/`), which
  shows only what has been ported. `ui = "gpui"` in `features.toml` (Nix:
  `programs.nova.ui`) makes novad run `nova-ui` after a restart; an unknown
  name or a missing `nova-ui` runs Quickshell with a warning. The GPUI side
  has no UI test suite: a change to it is shown with screenshots or a
  recording.
- A feature's ui reads novad through `nova-ui-client`
  (`framework-ui/client`, whose `lib.rs` shows each call), with its api
  crate's types: `Live::follow` for a topic's latest state as an entity its
  views observe, `Daemon::on_event` for events that are no state,
  `Daemon::call` and `Daemon::send` for commands. It never opens the socket
  itself. The client reconnects on its own and a `Live` keeps its last
  state while novad is away; state that must not outlive a connection (an
  open picker) is cleared by observing `Daemon::connection`.

## Layout

A crate's folder decides what it may use, and which side it is on. The
daemon side is everything novad is built from; the UI side is the only one
that may use GPUI: `app-ui/`, `framework-ui/` and each feature's `ui`.

- `framework/<name>/`: the shared part every feature plugs into. It never
  names a feature. Wire types that aren't a feature's sit in their own
  light `-info` crate (`app-info`, `daemon-info` for novad's own messages,
  `desktop-info`, `host-info`, `theme-info`: plain serde types, with their
  `Event` and `TOPIC` where they have one), so api crates and fronts carry
  them without the code that builds them. An `-info` crate uses only
  `nova-carrier` and other `-info` crates (rule I1).
- `features/<id>/api/`: the feature's own messages and types, what other
  crates may use. `features/<id>/core/`: its work, the part that talks to
  the system and draws nothing. `features/<id>/ui/`: what it draws, with
  GPUI. They sit beside the feature's `feature.toml`.
- `framework-ui/<name>/`: the UI side's shared part, which every feature's
  ui plugs into (the socket client, the look, the stage). It never names a
  feature.
- `fronts/<name>/`: other ways in, the clients and outside interfaces
  (`fronts/cli`: the `nova` command's socket commands). The socket server
  itself is part of the framework.
- `app/`: wiring only. It starts everything and lists the features. The
  `nova` binary (`app/src/bin/nova`) parses the command line and keeps the
  commands that run Nova itself (start, restart, quit, setup, doctor,
  config, claude-statusline, daemon).
- `app-ui/`: wiring only, for the UI side. The `nova-ui` binary starts GPUI
  and what plugs into it.

Each side is its own cargo workspace with its own `Cargo.lock`: the daemon
side's at the repo root, the UI side's at `app-ui/Cargo.toml`. GPUI brings
hundreds of crates, some in two versions, which novad's lock, its
cargo-deny rules and its static build never see this way, and a change to
the UI side leaves every daemon build as it was. A UI crate names its
workspace in `[package]` (`workspace = "../../app-ui"`, one more `../` for
a feature's ui) and uses the daemon side's api and `-info` crates by path.
The UI workspace's `[workspace.package]` and `[workspace.lints]` are copies
of the root's; `clippy.toml` and `rustfmt.toml` at the root hold for both.

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
| C2 | in those folders is a member of its side's workspace | add its folder or kind's glob to that workspace's `members` |
| C3 | under `features/<id>/` has a `feature.toml` beside it | name the folder after the feature's id |
| W1 | in `framework/` uses only other framework crates | let the feature plug in (a module, a port, a message type) |
| W2 | in `fronts/` uses only listed framework crates and api crates | send a message |
| I1 | named `framework/<name>-info` uses only `nova-carrier` and other `-info` crates | move the code to the crate that builds the types |
| F1 | `api` uses only listed framework crates and api crates | move the code to `core` |
| F2 | `core` uses only listed framework crates and api crates, never a `ui` | send an event |
| F3 | `ui` uses only listed framework crates and api crates, never a `core` | send a message |
| F4 | in a feature never uses another feature's `core` or `ui`, a front or the app | use its api, or a port in the framework |
| U1 | on the daemon side never uses a UI crate or GPUI | send an event |
| U2 | in `framework-ui/` uses only other `framework-ui` crates and listed framework crates, never a feature | let the feature's ui plug in |
| U3 | `app-ui` uses only ui and api crates, `framework-ui` and listed framework crates | send novad a message |
| S1 | workspace root has the same `[workspace.package]` and `[workspace.lints]` as the other | make the change in both |
| S2 | has `publish.workspace = true` (no Nova crate is published) | add the line |
| T1 | in a feature has no dev-dependency on another feature's `core` or `ui`, unless `DEV_EXCEPTIONS` lists the pair | use a fake from the framework, or list the pair with its reason |
| D1 | `Cargo.toml` has both allowed lists | add them |
| D2 | depends only on crates its allowed lists name | add it to the list and say why in the PR |

The framework crates each half may use are listed at the top of the
script. A framework crate joins a list only when every crate of that kind
may use it: a daemon-side part never on the `ui` list (which holds for
`framework-ui` and `app-ui` too), and a screen-side part goes in
`framework-ui/`, not on a list. Dev-dependencies are checked for T1 only.
Changing a rule or a list is a layout change: say so in the PR.

### Drawing with the look kit

`framework-ui/look` (`nova-look`) is the GPUI side's look. Its `lib.rs`
lists the pieces, and `examples/sample.rs` draws a pill and a panel with all
of them (`cargo run --manifest-path app-ui/Cargo.toml -p nova-look --example
sample`). Every ui crate draws with it:

- Sizes, opacities and timings are `nova_look::tokens`. A value only one
  feature uses is a constant in that ui crate's own `tokens` module. No
  literal in a view.
- Colors are `Theme::of(cx)`, read while rendering and never kept, so a
  theme change redraws everything with no code in the view.
- Text starts from `.body_text(cx)` on the root of what a window draws
  (the stage sets it for pills and panels; an overlay's window sets it
  itself): the face, size, weight, ink and Qt's line height
  (`tokens::LINE_HEIGHT`), so text sits as it does in the QML. A view
  changes only what differs and sets no line height.
- The shared tokens and the icon glyphs also live in `ui/theme/Tokens.qml`
  and `ui/theme/icons.js` until the Quickshell shell is removed. nova-look's
  tests hold the two equal: change a value in both.
- A window opens with `ShellWindow::options()` and nothing else: GPUI draws
  a window without focus at 30 frames a second at most, and a shell window
  never has focus. `just lint-traps` fails on `WindowOptions {` elsewhere.
- Glass is `Glass::new(radius).build(cx)`. The window's owner (the stage,
  for pills and panels) passes the shapes it draws to `set_blur_region`
  and to GPUI's `set_input_region` each frame they change, and leaves
  `tokens::SHADOW_ROOM` free around glass. Nothing clips there, the glass
  included: content that must be clipped to it goes in a child with
  `overflow_hidden`. A window draws at most 32 glasses with a shadow.
- GPUI clips children to a rounded parent's box, not its corners: content
  that reaches the edge rounds itself with `fit_corners`.
- Hover is GPUI's own `.hover()` and `on_hover`: both let go when the
  pointer leaves the window (a fix in the fork, IMP-435).
- Motion is `motion::ease`, `Eased` and `Spring`. Ask for the next frame
  only while a value hasn't settled.
- An icon is `Icon::new(Glyph::NAME)`; a new glyph joins the list in
  `icon.rs` (and `icons.js`, while it exists).
- One line of typed text is a `TextField` (caret, selection, clipboard,
  input methods, a placeholder and a grey completion); it emits
  `FieldEvent::Changed`. An app's icon is `AppIcon::new(id, name)
  .icon(path)`, a letter tile when there is no image. A choice between a
  few options is `Segments`.

### Porting a feature's UI to GPUI

The stage (`framework-ui/stage`, `nova-stage`) is the one see-through
window across the top of each screen that pills and panels are drawn on.
It draws the glass, lays out the bar's rows, grows a pill into its panel,
keeps the blur and click regions on what it drew, holds the keyboard while
a panel is open and closes panels on Esc and a click outside. A feature's
ui never does any of that, and opens no window for a pill or a panel. It
hands over views that draw content, registered by the view ids its
`feature.toml` slots already name.

**1. Registering views.** `features/<id>/ui` (`nova-<id>-ui`) has one
`pub fn register(cx: &mut App)`, and `app-ui/src/main.rs` calls it in
`register_features` (one line per feature, sorted by id; the crate joins
`app-ui`'s allowed list and the UI workspace's dependencies). `register`
only stores functions: the stage builds a view when a slot of a ready
feature asks for it, once per screen, and drops it when the feature goes.
A feature that is off or not ready costs nothing, so start nothing in
`register` itself; follow novad (`Live::follow`) inside the build function
or the view's constructor.

A pill in a bar row (hosts `bar.left`, `bar.center`, `bar.right`). The view
draws the pill's content at the bar's height; the stage draws the glass:

```rust
nova_stage::pill("resources.pill", cx, |_screen, cx| Pill::new(cx.new(ResourcesPill::new)));
```

A pill with a panel. A click on the pill opens it, and the panel's view
draws only its open look, filling `size`:

```rust
nova_stage::pill("media.pill", cx, |_screen, cx| {
    let panel = Panel::new("media", size(PANEL_WIDTH, MEDIA_HEIGHT), cx.new(MediaPanel::new));
    Pill::new(cx.new(MediaPill::new)).panel(panel)
});
```

A panel that is no pill's own (host `stage.panels`). `.from(view id)`
grows it out of another feature's pill; without it, it rises into place
and fades in where it stands, as the launcher does. `takes_typing` gives
the panel the keyboard and the focus each time it opens:

```rust
nova_stage::panel("launcher.panel", cx, |screen, cx| {
    let launcher = cx.new(|cx| Launcher::new(screen, cx));
    let (focus, size) = (launcher.read(cx).focus(cx), launcher.read(cx).size(cx));
    Panel::new("launcher", size, launcher).top(top(screen)).takes_typing(focus)
});
```

A part: a view for a host another feature draws (`central-pill.parts`,
`right-pill.buttons`, `quick-settings.tiles`). The host's owner keeps a
`Parts` per screen, calls `sync` when `Stage::slots` notifies and draws
`views()` in slot order:

```rust
// features/clock/ui
nova_stage::part("clock.time", cx, |_screen, cx| cx.new(ClockTime::new).into());
// features/workspaces/ui: round cells sit closer to the pill's ends than text
nova_stage::part("workspaces.list", cx, |screen, cx| {
    Part::new(cx.new(|cx| WorkspaceList::new(screen, cx))).inset(WORKSPACE_INSET)
});
// features/bar/ui, in the central pill's view
let parts = Parts::new("central-pill.parts", screen, cx);
```

A part draws its content only, edge to edge: the host adds what goes
between parts (the central pill's dividers) and keeps each end
`Part::inset` from the first and last part (`PAD` by default).

An overlay (host `shell.overlays`): something with windows of its own over
the screens, like pop-ups or the volume capsule. `build` runs once while
the feature is ready and returns what keeps it alive; the stage drops that
when the feature goes. It opens its windows with `ShellWindow::options()`
and follows `Stage::screens` for screens that come and go:

```rust
nova_stage::overlay("sound.capsule", cx, |cx| Box::new(cx.new(Capsule::new)));
```

An overlay's windows are its own, so it does for them what the stage does
for panels: it opens each one once per screen (deferred out of the update
that found the screen, and never at the start of a motion), keeps it only
as big as what it draws, and passes its shapes to `set_blur_region` and
`set_input_region` when they change. GPUI redraws a window whenever an
entity its last frame read notifies, so a window learns that it has nothing
to draw in a `cx.observe`, and then renders without reading that entity: at
rest it commits nothing. It resizes only while nothing moves: what is new
is laid out unseen, the window grows once, and the motion starts in the
frame after. `Stage::observe_open` calls it back
whenever a panel opens, closes or resizes (the pop-ups pause while quick
settings is open), and when `Stage::logs_frames` is set it logs its own
frames with a `FrameLog` named `<screen>/<feature>`, as the stage does.
`features/notifications/ui` (`popups.rs`, `window.rs`) is the worked example.

**2. What holds for every view on the stage.**

- A panel's name is its feature's id, the name `nova toggle` uses. Code
  opens and closes one with `Stage::toggle`, `open`, `close` and
  `is_open`, by name and screen. A pill whose own buttons decide what a
  click does (the central pill) is built with `.opens_on_click(false)` and
  calls `Stage::toggle` itself.
- The contract's flags are on `Panel`: `.exclusive(false)` to stay open
  beside the others, `.closes_on_click_outside(false)`, `.takes_typing`,
  `.top` and `.radius` for a panel that isn't the usual shape.
- A panel changes size only with `Stage::resize(name, screen, size, cx)`,
  open or not; an open one eases there. The view never sizes its own glass.
- A view redraws by itself (`cx.notify()` when what it shows changed) and
  repaints only itself: the stage's own drawing reads no feature state. It
  asks for frames only while one of its values is moving. `nova-ui
  --frames` logs the frames each screen's stage drew: nothing for a shell
  at rest, one a second for a clock that shows seconds, and for each
  motion how many of its frames came late.
- A pill that has nothing to show at times passes a `Shown` with
  `Pill::shown_while` and sets it as that changes: hidden, it takes no
  room and no region, and a panel from it rises into place instead.
- The bar's center row is centered, unless a view in it wraps one thing in
  `nova_stage::middle(...)` (the clock part's time): then that thing's
  middle is the screen's middle, and the row grows to the left of it.
- A `Screen` gives a view its screen's `id` (for `Stage` calls), `name`
  (the output name novad's messages use), `size` and `refresh`.
- A feature's ui uses its own api crate and other features' api crates,
  never another feature's ui (rule F4): what one draws inside another
  comes in as a part.

`nova-ui --demo` puts three test pills and two test panels on the stage
with no feature ported (`app-ui/src/demo.rs`, a short worked example of
all of the above but parts); `--demo-toggles <n>` toggles one
`n` times and quits.

**3. The files a port touches.** `features/launcher/ui` is the worked
example: read it beside `ui/launcher/*.qml` first. A port draws what the
QML draws from the same messages: novad, the api crate and
`feature.toml` stay as they are, and the QML stays until the Quickshell
shell is removed.

| File | What goes in |
|---|---|
| `features/<id>/ui/Cargo.toml` | `nova-<id>-ui`, `workspace = "../../../app-ui"`, `publish.workspace = true`, both allowed lists (rule D1) |
| `features/<id>/ui/src/lib.rs` | `pub fn register`, and nothing else that runs |
| `features/<id>/ui/src/tokens.rs` | the feature's own sizes, with the test that holds each equal to `ui/theme/Tokens.qml` |
| a module with no GPUI in it (`rows.rs`) | what the QML's `.js` worked out (which rows, their order, the selection, the scroll), with unit tests |
| the view (`launcher.rs`) and its drawing (`draw.rs`) | state and what novad is asked in one, `impl Render` in the other |
| `app-ui/Cargo.toml` | the crate in the allowed list, in `[dependencies]` and in `[workspace.dependencies]`, with any api crate it is the first to use |
| `app-ui/src/main.rs` | one line in `register_features` |
| `app-ui/Cargo.lock` | what the build writes |
| `framework-ui/look` | a piece a second feature will draw too, with its tokens in both `tokens.rs` and `Tokens.qml` |

Other ports are open at once and all add lines to the two `app-ui` files:
merge main before every push and keep every line, sorted.

**4. Patterns to copy** (each is in the launcher's ui):

- State from novad is a `Live::follow(TOPIC, cx, pick)` the view keeps,
  with `cx.observe` on it. A question is `Daemon::call::<Reply>(command,
  cx)`, its task kept in the view so a newer question drops the older
  one; a command with nothing to wait for is `Daemon::send`.
- The stage tells a view nothing when its panel opens. The view reads
  `Stage::is_open` in `render` (the stage redraws its views when a panel
  opens or closes), compares it with what it saw last, and changes its
  state in `cx.defer_in`. It resets after closing only once the panel has
  faded (`DURATION_PANEL + DURATION_FADE`), so the fade shows what was
  open.
- Content that changes height computes the panel's size from its state
  and calls `Stage::resize` in `cx.defer`: the stage reads the view to lay
  the panel out, so a call inside the view's own update panics.
- Keys are one `on_key_down` on the view's root. It handles its keys and
  calls `cx.stop_propagation()`; every other key is left alone, so Esc
  reaches the stage. A `TextField` child takes its own keys first and
  lets Up, Down, Enter and Tab through.
- A long list draws only the rows in view, placed by a layout the pure
  module computed, with `on_scroll` moving an offset the view keeps. The
  selection's highlight is one `Eased` the rows sit above.
- Hover is `group(name)` on the row and `group_hover(name, ...)` on what
  appears. A button inside a clickable row calls `cx.stop_propagation()`
  in its own mouse-down.
- Until IMP-435 lands in the GPUI fork, hover sticks when the pointer
  leaves a window: `.hover()` and `on_hover` work from where the pointer
  last was. At a window's edge, follow `on_mouse_move` and the window's
  own leave as `Popups::pointer` in `features/notifications/ui` does.
  Delete this note with that fix.
- What another feature adds comes through its slots: `Slots::for_host`
  says which views a host has (the launcher shows its Files tab while
  `launcher.providers` has `files`), with `cx.observe` on `Stage::slots`.
- An output is named to novad by `screen.name`; `screen.id` is only for
  `Stage` calls.

**5. Checking a port.** There is no UI test suite: `just ui-check` covers
the pure module and the tokens, and the rest is seen.

- `nova-ui --frames` beside a real `nova daemon` (`NOVA_SOCKET`, a
  `features.toml` with only this feature on): open, use and close the
  panel, and read the log for late frames and for frames at rest.
- On screen in a cloud thread: sway with `WLR_BACKENDS=headless`, the
  lavapipe Vulkan driver, `grim` for shots, `wtype` for keys,
  `wf-recorder` for the recording. Three things there are the tools, not
  the port: `swaymsg` needs `SWAYSOCK`; the first key of each `wtype`
  call is lost (lead with `-k Shift_L`); clicks and the wheel from
  `wlrctl` never reach nova-ui, so say in the PR that they wait for a
  real pointer.
- Today's look for the comparison: a small `.qml` that loads the
  feature's component with canned data, run by `qmlscene` with
  `QT_QPA_PLATFORM=offscreen` and `grabToImage`. Name no property `data`
  (it is `Item`'s own).
- The PR carries a shot beside today's shell in each state, a recording
  of the panel in use, and a list headed "Differences from today's
  shell" with everything that is not the same, wanted or not.

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
  and referenced everywhere else: design values in `theme/Tokens.qml` (the
  GPUI side: `nova_look::tokens`), wire
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
| No `unwrap`, `expect`, `panic`, `todo` or `unimplemented` outside tests | Clippy restriction lints; `clippy.toml` exempts tests |
| Small functions: at most 5 arguments | Clippy `too_many_arguments` (`clippy.toml`) |
| Small functions: at most 80 lines | Clippy `too_many_lines` (`clippy.toml`) |
| Shallow code: blocks nest at most 5 deep | Clippy `excessive_nesting` (`clippy.toml`) |
| Small files: a Rust file has at most 800 lines | `scripts/check-file-length.sh` (`just file-length`, flake check `file-length`) |
| No polling: a wait is on a channel or event, with a timeout | Clippy `disallowed-methods` (`std::thread::sleep`); retries against what sends no event wait in `nova_base::backoff::wait_out`, tests in `nova_test_support::settle` |

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

Run `just` inside `nix develop` to list the recipes. CI runs the flake checks
on every push: what `just check` runs plus the headless e2e, so locally
`just check` and `just e2e` cover it. Before opening a PR, also run
`just test-sandbox`: it runs the Rust tests in CI's Nix sandbox, where a test
that needs the network, setuid bits or a host tool fails although
`cargo test` passes (cloud sessions can't fetch its GitHub inputs, so there CI
is the first sandbox run). `just run` logs to `~/.local/state/nova-dev`.

The UI side has its own shell and recipes, since GPUI needs libraries the
default shell leaves out: `nix develop .#ui`, then `just ui-build`
(`app-ui/target/debug/nova-ui`) and `just ui-check` (rustfmt, clippy, unit
tests, cargo-deny), which a change to the UI side runs before its PR;
`just check` covers the layout and file lengths of both sides. A cloud
session can build it too: the first build takes about five minutes on four
cores and a few GB of disk. Without a compositor `nova-ui` starts and prints
its start line but opens no window; anything on screen is shown on the
desktop, off the maintainer's screen like any other test run. To run it
under a checkout's novad, pass `nova daemon --gpui-ui <path to nova-ui>`
with `ui = "gpui"` in the features.toml it reads. By hand, as a second
client of a running novad, set `NOVA_SOCKET` to novad's socket. `nova-ui
--demo` shows test pills and panels on the stage with no feature ported, and
`--frames` logs the frames it draws. `nova-ui
--replay <folder>` talks to a fake novad that replays the folder's
recordings where the real one can't run: a recording is `<topic>.jsonl`,
what `nova ipc subscribe '{"topics": ["<topic>"]}'` printed on a real
desktop, kept in the feature's `ui/recorded/`. CI builds and checks it
in the flake checks named `ui-*`, and a PR that changes files only under
`app-ui/`, `framework-ui/` or `features/<id>/ui/` skips the QML checks.

Every change goes through a pull request; `main` is protected.

A stack of PRs is one chain: the bottom PR targets main, each one above
targets the branch below, and a branch merges only its own base.
`scripts/stack.sh status|restack` checks the
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
output); never open a nested compositor window, which lands on the maintainer's workspace (on
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
