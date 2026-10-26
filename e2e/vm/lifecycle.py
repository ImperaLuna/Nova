# Nova under systemd and a real compositor, driven through the `nova`
# command the way a user would. This part starts the session and checks Nova
# came up; session.py or frames.py follows it, each in a VM of its own, so
# the two halves run side by side. default.nix defines USER, UID, PASSWORD,
# COMPOSITOR, QUIT_COMPOSITOR, FOCUS_NEXT_WORKSPACE, FULLSCREEN_WINDOW,
# CLOSE_WINDOW, LIST_WINDOWS, LIST_LAYERS, ADDED_OUTPUTS (the commands that
# add an output and remove it again, as one pair, or none where the
# compositor can't), COMPOSITOR_LOG and FRAMES_REPORT above this script.
import collections
import datetime as dt
import itertools
import json
import re
import shlex
import statistics

UNIT = "nova.service"
RUNTIME_DIR = f"/run/user/{UID}"
NOVA_PROCESSES = "pgrep -x 'Nova|NovaUI'"
WAIT = dt.timedelta(minutes=2)
# Booting, logging in and starting the compositor.
SESSION_START = dt.timedelta(minutes=10)
# What the bar logs once it has drawn itself (ui/bar/Bar.qml).
BAR_SHOWN = "nova: bar shown"
# What the bar logs once the AI usage pill is on it (ui/bar/Bar.qml).
AI_USAGE_SHOWN = "nova: ai usage pill shown"
# A point inside that pill: it sits Tokens.barMargin (8) from the screen's
# left edge and Tokens.barGap (4) from its top, and is Tokens.barHeight (36)
# tall.
AI_USAGE_PILL = (24, 22)
# What the pill logs when its panel opens or closes (ui/ai_usage/AiUsage.qml).
AI_USAGE_OPENED = "nova: ai usage panel opened"
AI_USAGE_CLOSED = "nova: ai usage panel closed"
# What the bar logs when it gives its space to a fullscreen window, and
# when it takes it back (ui/bar/Bar.qml).
BAR_SPACE_GIVEN = "nova: bar space given to a fullscreen window"
BAR_SPACE_KEPT = "nova: bar space kept"
# Clicks a test gives one target before failing. A lost click would
# otherwise show only as a screenshot diff; the known cause is handled by
# the pointer wake-up below, but niri's side of it isn't understood.
CLICK_TRIES = 3
# How long a click's effect may take to show in the UI's log.
CLICK_EFFECT = dt.timedelta(seconds=10)
# Where the pointer waits between clicks, away from the bar.
POINTER_REST = (640, 700)
# How long niri gets after the session's first pointer move before a click.
# niri at times sends the first pointer input after it starts to no surface,
# so a test's first click would be lost (IMP-48).
POINTER_WAKE_SECS = 1
YDOTOOL = "YDOTOOL_SOCKET=/run/ydotoold/socket ydotool"
# How long the screen must stay the same before a screenshot.
SETTLED_SECS = 3
# How long to watch that a stopped unit stays stopped: past its RestartSec.
STAYS_OFF_SECS = 3
# How long an idle UI must commit nothing to its surfaces. Long enough for
# niri's once-a-second callbacks to a covered surface (IMP-38).
IDLE_SECS = 5
# Lines of `nova logs` history that reach back to the start of a run.
LOG_HISTORY = 1_000_000
# novad's log files: nova.log and the earlier ones, nova.log.1 to .5
# (logger.rs KEPT_RUNS), and the line each novad run starts with.
LOG_DIR = "~/.local/state/nova"
KEPT_LOGS = 5
RUN_START = " starting (pid "
# A frame the UI hands the compositor, in its WAYLAND_DEBUG lines.
SURFACE_COMMIT = r"wl_surface#[0-9]*\.commit("
# The same line, with libwayland's timestamp and the surface, after
# `nova logs`'s own time, level and source. libwayland 1.26 stamps the wall
# clock as [HH:MM:SS.micros]; older ones stamp milliseconds as [ms.frac].
TIMED_COMMIT = re.compile(r"\[\s*(?:(\d+):(\d+):)?(\d+(?:\.\d+)?)\].*-> wl_surface#(\d+)\.commit\(")
# A surface asking for a frame callback, and the compositor sending one.
FRAME_REQUEST = re.compile(r"-> wl_surface#(\d+)\.frame\(new id wl_callback#(\d+)\)")
FRAME_DONE = re.compile(r"\] .*?\bwl_callback#(\d+)\.done\(")
# Qt's own timing of each frame it renders (qt.scenegraph.time.renderloop):
# sync is Nova's QML handing the scene over, render the (software) GPU work.
QT_FRAME = re.compile(r"syncAndRender: frame rendered in (\d+)ms.*\bsync=(\d+), render=(\d+)")
# How long the panel may go on drawing after its pill logs that it opened or
# closed. The longest animation, the meters filling, ends at
# Tokens.durationPanel * 0.6 + Tokens.durationFill (750 ms) after opening;
# the rest is room for software rendering's late last frame.
MORPH_DRAWS_FOR = dt.timedelta(milliseconds=1200)
# The VM's outputs refresh at 60 Hz, for the frames report.
REFRESH_HZ = 60
# Frames a surface may draw beyond one per frame callback the compositor sent
# it: the first, which the click starts, and one whose callback was asked for
# before the log was followed. Counted per callback rather than per refresh:
# the VM has no real vblank, and niri paces callbacks faster than 60 Hz while
# a full-screen surface such as the click catcher is mapped (IMP-72).
FRAMES_BEYOND_CALLBACKS = 2
# How many surfaces may draw while the panel opens or closes: the stage on
# the pill's screen, and the click catcher, which closes the panel on a click
# outside it, as it starts or stops catching (ui/stage/ClickCatcher.qml draws
# only then). The bar's exclusion zone used to draw a frame too, each time an
# animation started (IMP-49).
MORPH_SURFACES = 2
# How many times the launcher opens and closes over a fullscreen window with
# WAYLAND_DEBUG on, for a stage that stays on Overlay (IMP-116).
LAYER_ROUNDS = 5
# The placeholder panels off the stage (ui/demo/DemoPanels.qml, on with
# NOVA_DEMO_PANELS=1), opened over Quickshell's IPC.
DEMO_PANELS = ("demo-sidebar", "demo-tab", "demo-center")
# What the volume capsule logs as it comes out and once it's gone
# (ui/volume/VolumeOsd.qml).
VOLUME_SHOWN = "nova: volume capsule shown"
VOLUME_HIDDEN = "nova: volume capsule hidden"
# The UI has the default sink's starting volume, so a change from here shows.
VOLUME_FOUND = "nova: volume output found"
# What quick settings logs (ui/quick_settings/QuickSettings.qml).
QUICK_SETTINGS_OPENED = "nova: quick settings opened"
QUICK_SETTINGS_CLOSED = "nova: quick settings closed"
# On the output ADDED_OUTPUTS adds, right of the VM's 1280x800 one.
ON_ADDED_OUTPUT = (1280 + 300, 300)
# What Quickshell logs first in each UI process.
UI_LAUNCHED = "Launching config"
# The VM's default output (a null sink, default.nix), as wpctl names it.
WPCTL_SINK = "wpctl {} @DEFAULT_AUDIO_SINK@"
# A point on the capsule: Tokens.capsuleWidth (48) wide, Tokens.capsuleEdgeGap
# (12) from the right edge of the 1280x800 screen, centered vertically.
CAPSULE = (1280 - 12 - 24, 400)
# Longer than the capsule stays out after a change (Tokens.durationCapsuleHold).
CAPSULE_HOLD_SECS = 3
# Long enough for it to slide out (Tokens.springCapsule), well inside
# Tokens.durationCapsuleHold.
CAPSULE_SLIDE_SECS = 0.6
# A window's place and size in the compositor's window list.
GEOMETRY_KEYS = ("layout", "at", "size")
# Inside the center placeholder: it's 640x400 in the middle of the 1280x800
# screen. POINTER_REST is below it.
DEMO_CENTER_INSIDE = (640, 400)
# How long a click that shouldn't close the center placeholder is given to
# do so anyway.
NO_EFFECT_SECS = 2
# Esc, pressed and released, as ydotool names keys (Linux key codes).
ESC = "1:1 1:0"
ENTER = "28:1 28:0"
# What the lock screen logs (ui/lock).
SESSION_LOCKED = "nova: session locked"
SESSION_UNLOCKED = "nova: session unlocked"
WRONG_PASSWORD = "nova: lock screen: wrong password"
# What the launcher logs (ui/launcher).
LAUNCHER_OPENED = "nova: launcher opened"
LAUNCHER_CLOSED = "nova: launcher closed"
# Keys typed before this go to the window that had the keyboard.
LAUNCHER_TYPING = "nova: launcher has the keyboard"
# The systemd service novad starts an app as (crates/novad/src/launcher/launch.rs).
FOOT_UNIT = "app-nova-foot-"
# What the power menu logs (ui/power), and its key for Lock.
POWER_OPENED = "nova: power menu opened"
POWER_CLOSED = "nova: power menu closed"
KEY_L = "38:1 38:0"
# novad's sleep inhibitor (crates/novad/src/logind.rs) as `systemd-inhibit
# --list` shows it: who, what, why and mode.
NOVA_INHIBITOR = "Nova.*sleep.*Locking the screen before sleep.*delay"
# The panels `nova toggle` opens, with what each logs once open and once
# closed, for the windows-don't-move check and the Pss of each state. The
# tray's icons log nothing and draw only inside the right pill.
REAL_PANELS = {
    "launcher": (LAUNCHER_OPENED, LAUNCHER_CLOSED),
    "ai-usage": (AI_USAGE_OPENED, AI_USAGE_CLOSED),
    "power": (POWER_OPENED, POWER_CLOSED),
    "quick-settings": (QUICK_SETTINGS_OPENED, QUICK_SETTINGS_CLOSED),
}
# Where the Pss of each state is saved, next to the frames test's result.
PSS_REPORT = "/tmp/pss.txt"
# How long a panel is given to finish opening or closing before windows are
# compared and Pss is read: longer than its animations (MORPH_DRAWS_FOR),
# so a window the compositor moves late is caught too.
PANEL_SETTLE_SECS = 2
# The window the tests open, and how it shows in novad's focused window.
FOOT = "foot"
NOVA_SEES_FOOT = f"nova status --json | grep -qF '\"app_id\":\"{FOOT}\"'"


def as_user(command: str) -> str:
    """A shell command that runs `command` as the logged-in user, with the
    session's runtime dir and Wayland display, as in a terminal there."""
    env = (
        f"export XDG_RUNTIME_DIR={RUNTIME_DIR}; "
        f"export WAYLAND_DISPLAY=$(cd {RUNTIME_DIR} && ls wayland-* | grep -v lock | head -n1); "
    )
    return f"su -l {USER} -c {shlex.quote(env + command)}"


def status(socket: str | None = None) -> dict | None:
    flag = f" --socket {socket}" if socket else ""
    code, out = machine.execute(as_user(f"nova status --json{flag}"))
    return json.loads(out) if code == 0 else None


def wait_running(not_daemon=None, not_ui=None, socket=None) -> dict:
    """Waits until novad reports a running UI, both with pids other than the
    given ones, and returns its status."""
    found = {}

    def ready(_last_try: bool) -> bool:
        current = status(socket)
        if not current or current["ui"]["state"] != "running":
            return False
        if current["pid"] == not_daemon or current["ui"]["pid"] == not_ui:
            return False
        found.update(current)
        return True

    retry(ready, timeout=WAIT)
    return found


def unit_property(name: str) -> str:
    _, out = machine.systemctl(f"show {UNIT} --property={name} --value", user=USER)
    return out.strip()


def process_name(pid: int) -> str:
    return machine.succeed(f"cat /proc/{pid}/comm").strip()


def screenshot(state: str) -> None:
    """Saves <state>.png once the bar has drawn itself and the picture has
    stopped changing. grim asks the compositor for its output, which is
    reliable where the emulated GPU's scanout isn't always up to date."""
    machine.wait_until_succeeds(as_user(f"nova logs --no-follow --ui | grep -qF {shlex.quote(BAR_SHOWN)}"), timeout=WAIT)
    path = f"/tmp/{state}.png"
    last_hash = [""]
    unchanged = [0]

    def settled(_last_try: bool) -> bool:
        current = machine.succeed(as_user(f"grim {path} && sha256sum {path}"))
        unchanged[0] = unchanged[0] + 1 if current == last_hash[0] else 0
        last_hash[0] = current
        return unchanged[0] >= SETTLED_SECS

    # retry() calls this once a second.
    retry(settled, timeout=WAIT)
    machine.copy_from_machine(path)


def pointer_to(x: int, y: int) -> str:
    """The command that moves ydotool's virtual mouse to (x, y). Both
    compositors run it with flat acceleration, so a move is 1:1. An absolute
    move passes through (0, 0), so niri's hot corner is off."""
    return f"{YDOTOOL} mousemove --absolute -x {x} -y {y}"


def click(x: int, y: int) -> None:
    """Clicks at (x, y) on the screen with ydotool's virtual mouse, then
    moves the pointer out of the way so no hover shows in screenshots."""
    machine.succeed(f"{pointer_to(x, y)} && {YDOTOOL} click 0xC0 && {pointer_to(*POINTER_REST)}")


def ui_log_count(line: str) -> int:
    """How often the UI logged `line` in this novad run. The whole run: with
    WAYLAND_DEBUG on, a click logs far more than `nova logs` shows by
    default, and the run's file can pass novad's 2 MB and move on to
    nova.log.1 mid-subtest, where `nova logs` no longer reads it. So the
    count reads back through the files to the one this novad started in."""
    files = " ".join(["nova.log"] + [f"nova.log.{runs_ago}" for runs_ago in range(1, KEPT_LOGS + 1)])
    out = machine.succeed(
        as_user(
            f"cd {LOG_DIR} && for f in {files}; do [ -f $f ] || break; "
            f"grep -F ' ui: ' $f | grep -cF {shlex.quote(line)}; grep -qF {shlex.quote(RUN_START)} $f && break; done; true"
        )
    )
    return sum(int(count) for count in out.split())


# Hyprland's numbered layers, by the names niri uses.
HYPRLAND_LEVELS = {"0": "background", "1": "bottom", "2": "top", "3": "overlay"}


def layers_of(namespace: str) -> set[str]:
    """The layers (lowercase) the compositor has a surface named `namespace` on."""
    listed = json.loads(machine.succeed(as_user(LIST_LAYERS)))
    if isinstance(listed, list):
        return {surface["layer"].lower() for surface in listed if surface["namespace"] == namespace}
    return {
        HYPRLAND_LEVELS[level]
        for monitor in listed.values()
        for level, surfaces in monitor["levels"].items()
        for surface in surfaces
        if surface["namespace"] == namespace
    }


def assert_layers(layer: str, *namespaces: str) -> None:
    """Every surface named one of `namespaces` comes to sit on `layer`."""

    def there(last_try: bool) -> bool:
        found = {namespace: layers_of(namespace) for namespace in namespaces}
        if last_try:
            print(f"wanted {namespaces} on {layer}, found {found}")
        return all(on == {layer} for on in found.values())

    retry(there, timeout=CLICK_EFFECT)


def window_geometry() -> list:
    """Where each window is and how big, as the compositor lists them
    (niri's `layout`, Hyprland's `at` and `size`), and nothing else: a panel
    that takes typing moves the focus, but must move no window."""
    windows = json.loads(machine.succeed(as_user(LIST_WINDOWS)))
    return [{key: window[key] for key in GEOMETRY_KEYS if key in window} for window in windows]


def pss_mb(pid: int) -> str:
    """A process's proportional memory (Pss) in MB to one decimal, as
    e2e/run.sh reads it."""
    kb = machine.succeed(f"awk '/^Pss:/ {{ print $2 }}' /proc/{pid}/smaps_rollup").strip()
    return f"{int(kb) / 1024:.1f}"


def toggle_moves_no_window(command: str, line: str, windows: list, what: str) -> None:
    """Runs `command`, which opens or closes a panel, waits until the UI
    logs `line` and then for the panel to settle, and fails if any window
    moved or resized from `windows` (window_geometry())."""
    before = ui_log_count(line)
    machine.succeed(as_user(command))
    retry(lambda _last_try: ui_log_count(line) > before, timeout=CLICK_EFFECT)
    machine.sleep(PANEL_SETTLE_SECS)
    assert window_geometry() == windows, f"windows moved with {what}"


def demo_logged(panel: str, state: str) -> str:
    """What a placeholder panel logs when it's "opened" or "closed"."""
    return f"nova: demo panel {panel} {state}"


def layers_follow_the_launcher(rounds: int) -> None:
    """Opens and closes the launcher over the fullscreen window `rounds`
    times, checking each time that Nova moves to Overlay and back to Top.
    Once on Hyprland the stage stayed on Overlay after the launcher closed
    (IMP-116), rarely, so this repeats it with WAYLAND_DEBUG on and, if it
    happens again, prints what the UI sent after Esc: whether a set_layer
    went out, and which surfaces committed after it."""
    unit = "layers-log"
    capture = f"{RUNTIME_DIR}/{unit}.log"
    machine.succeed(as_user(f"systemd-run --user --collect --unit={unit} sh -c 'exec nova logs --ui -n 0 > {capture}'"))
    machine.wait_until_succeeds(as_user(f"systemctl --user is-active --quiet {unit}"), timeout=WAIT)
    try:
        for done in range(rounds):
            before = ui_log_count(LAUNCHER_OPENED)
            machine.succeed(as_user("nova toggle launcher"))
            retry(lambda _last_try: ui_log_count(LAUNCHER_OPENED) > before, timeout=CLICK_EFFECT)
            assert_layers("overlay", "nova-panel", "nova-stage", "nova-click-catcher")
            press_until_logged(ESC, LAUNCHER_CLOSED)
            try:
                assert_layers("top", "nova-stage", "nova-click-catcher")
            except Exception:
                lines = machine.succeed(f"cat {capture}").splitlines()
                closed = max(i for i, line in enumerate(lines) if LAUNCHER_CLOSED in line)
                after = lines[closed:]
                print(f"round {done + 1} of {rounds}: what the UI sent after {LAUNCHER_CLOSED!r}:")
                for line in after:
                    if "set_layer(" in line or "get_layer_surface(" in line:
                        print(f"  {line}")
                commits = collections.Counter(match.group(4) for line in after if (match := TIMED_COMMIT.search(line)))
                print(f"  commits after it, per wl_surface: {dict(commits)}")
                raise
    finally:
        machine.succeed(as_user(f"systemctl --user stop {unit}"))


def press_until_logged(keys: str, line: str) -> None:
    """Presses `keys` (ydotool's code:state pairs) until the UI logs `line`
    once more than before, like click_until_logged."""
    before = ui_log_count(line)
    for _ in range(CLICK_TRIES):
        machine.succeed(f"{YDOTOOL} key {keys}")
        try:
            retry(lambda _last_try: ui_log_count(line) > before, timeout=CLICK_EFFECT)
            return
        except Exception:
            print(f"no {line!r} after pressing {keys}, pressing again")
    raise AssertionError(f"pressing {keys} {CLICK_TRIES} times never logged {line!r}")


def click_until_logged(x: int, y: int, line: str, count=ui_log_count) -> None:
    """Clicks at (x, y) until the UI logs `line` once more than before, as
    `count` counts the UI's lines."""
    before = count(line)
    for _ in range(CLICK_TRIES):
        click(x, y)
        try:
            retry(lambda _last_try: count(line) > before, timeout=CLICK_EFFECT)
            return
        except Exception:
            print(f"no {line!r} after a click at ({x}, {y}), clicking again")
    raise AssertionError(f"{CLICK_TRIES} clicks at ({x}, {y}) never logged {line!r}")


def surface_commits(secs: int) -> int:
    """The surface commits the UI makes over the next `secs` seconds, from
    its WAYLAND_DEBUG lines as novad logs them."""
    follow = f"timeout {secs} nova logs --ui -n 0 > /tmp/ui-wayland.log; grep -c {shlex.quote(SURFACE_COMMIT)} /tmp/ui-wayland.log || true"
    commits = int(machine.succeed(as_user(follow)).strip() or 0)
    # What else the UI and the compositor said meanwhile, for when it fails.
    print(f"{commits} commits in {secs} s; Wayland lines by kind:")
    print(machine.succeed("grep -o '[a-z_]*#[0-9]*\\.[a-z_]*' /tmp/ui-wayland.log | sed 's/#[0-9]*//' | sort | uniq -c || true"))
    return commits


def assert_idle(what: str) -> None:
    """Waits for the UI to go a whole IDLE_SECS without drawing a frame."""
    counts: list[int] = []

    def idle(_last_try: bool) -> bool:
        counts.append(surface_commits(IDLE_SECS))
        return counts[-1] == 0

    try:
        retry(idle, timeout=WAIT)
    except Exception:
        raise AssertionError(f"the UI kept drawing {what}: commits per {IDLE_SECS} s: {counts}")


def morph_frames(what: str, logged: str) -> None:
    """Opens or closes the panel by clicking its pill until it logs
    `logged`, waits for the UI to stop drawing, and checks the frames it drew
    after that line: on at most MORPH_SURFACES surfaces, at most
    one per frame callback (FRAMES_BEYOND_CALLBACKS), all within
    MORPH_DRAWS_FOR. Prints how
    far apart they came (the frame time a user sees) and Qt's own time per
    frame; software rendering makes those a trend to read, not something to
    fail on."""
    # Followed live: novad keeps only its latest RECENT_ENTRIES lines for
    # `nova logs --no-follow`, which WAYLAND_DEBUG overruns in a frame or two.
    unit = f"morph-log-{what}"
    capture = f"{RUNTIME_DIR}/{unit}.log"
    machine.succeed(as_user(f"systemd-run --user --collect --unit={unit} sh -c 'exec nova logs --ui -n 0 > {capture}'"))
    machine.wait_until_succeeds(as_user(f"systemctl --user is-active --quiet {unit}"), timeout=WAIT)

    def captured(line: str) -> int:
        return int(machine.succeed(f"grep -cF {shlex.quote(line)} {capture} || true").strip() or 0)

    try:
        click_until_logged(*AI_USAGE_PILL, logged, count=captured)
        assert_idle(f"after the panel {what}")
    finally:
        machine.succeed(as_user(f"systemctl --user stop {unit}"))
    lines = machine.succeed(f"cat {capture}").splitlines()
    after_logged = lines[next(i for i, line in enumerate(lines) if logged in line) :]
    frames: dict[str, list[float]] = {}
    # Each callback asked for, by its id, the surface that asked; and how
    # many each surface was sent.
    asked: dict[str, str] = {}
    callbacks: dict[str, int] = {}
    for line in after_logged:
        if match := FRAME_REQUEST.search(line):
            surface, callback = match.groups()
            asked[callback] = surface
        elif "->" not in line and (match := FRAME_DONE.search(line)):
            if surface := asked.pop(match.group(1), None):
                callbacks[surface] = callbacks.get(surface, 0) + 1
        if match := TIMED_COMMIT.search(line):
            hours, minutes, seconds, surface = match.groups()
            if hours is None:
                ms = float(seconds)
            else:
                ms = ((int(hours) * 60 + int(minutes)) * 60 + float(seconds)) * 1000
            frames.setdefault(surface, []).append(ms)
    assert frames, f"no frames drawn after the panel {what}"
    print(f"panel {what}: frames per surface:")
    for surface, times in frames.items():
        gaps = [b - a for a, b in itertools.pairwise(times)]
        late = sum(gap > 2000 / REFRESH_HZ for gap in gaps)
        timing = f", {statistics.median(gaps):.1f} ms apart (longest {max(gaps):.1f} ms, {late} late)" if gaps else ""
        print(f"  wl_surface#{surface}: {len(times)} frames over {times[-1] - times[0]:.0f} ms{timing}")
    # `just frames`'s report of the same frames (ui/stage/FrameLog.qml): it
    # fails when the UI logged none. Software rendering misses vblanks
    # anyway, so no budget; on a busy runner every frame can come late,
    # which the report calls nothing animated, though the UI logged them.
    status, report = machine.execute(f"{FRAMES_REPORT} -v refresh='*={REFRESH_HZ}' {capture}")
    print(report)
    assert status == 0 or "nothing animated" in report, f"the frame report failed after the panel {what}"
    qt = [tuple(map(int, match.groups())) for line in after_logged if (match := QT_FRAME.search(line))]
    if qt:
        for name, values in zip(("frame", "sync", "render"), zip(*qt)):
            print(f"  Qt {name} time: median {statistics.median(values)} ms, longest {max(values)} ms over {len(values)} frames")
    assert len(frames) <= MORPH_SURFACES, f"{len(frames)} surfaces drew after the panel {what}, at most {MORPH_SURFACES} may"
    budget_ms = MORPH_DRAWS_FOR / dt.timedelta(milliseconds=1)
    # From the first frame any surface drew, so one that starts late counts.
    first = min(times[0] for times in frames.values())
    for surface, times in frames.items():
        drew_for = times[-1] - first
        assert drew_for <= budget_ms, f"wl_surface#{surface} drew until {drew_for:.0f} ms after the panel {what}'s first frame, at most {budget_ms:.0f} ms"
        sent = callbacks.get(surface, 0)
        assert len(times) <= sent + FRAMES_BEYOND_CALLBACKS, f"wl_surface#{surface} drew {len(times)} frames after the panel {what} but was sent {sent} frame callbacks: it drew without waiting for them"


def open_foot(unit: str) -> None:
    """Opens a foot window and waits until the compositor lists it, then
    until novad reports it as the focused window. foot runs as a transient
    user unit, so it doesn't hang off the `su` that starts it and its output
    lands in the journal; on a timeout, what each side saw is printed. Not
    with the user manager's WAYLAND_DEBUG, which is there for the UI."""
    machine.succeed(
        as_user(
            f"systemd-run --user --collect --unit={unit} "
            f"--setenv=WAYLAND_DISPLAY=$WAYLAND_DISPLAY --setenv=WAYLAND_DEBUG= {FOOT}"
        )
    )
    try:
        machine.wait_until_succeeds(as_user(f"{LIST_WINDOWS} | grep -qF '\"{FOOT}\"'"), timeout=WAIT)
    except Exception:
        print(f"{COMPOSITOR} never listed the {FOOT} window")
        print_window_state(unit)
        raise
    try:
        machine.wait_until_succeeds(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    except Exception:
        print(f"{COMPOSITOR} lists the {FOOT} window, but novad doesn't report it focused")
        print_window_state(unit)
        raise


def print_window_state(unit: str) -> None:
    """What foot, the compositor and novad say, for when a window wait fails."""
    # The journal as root: the user's own may not hold their units' lines.
    print(machine.execute(f"journalctl --user-unit={unit} --no-pager")[1])
    for command in (
        f"systemctl --user status {unit} --no-pager",
        LIST_WINDOWS,
        COMPOSITOR_LOG,
        "nova status --json",
        "nova logs --no-follow -n 100",
    ):
        print(f"$ {command}")
        print(machine.execute(as_user(command))[1])


def wait_gone(pid: int) -> None:
    machine.wait_until_fails(f"test -e /proc/{pid}", timeout=WAIT)


def assert_stays_off(restarts: str) -> None:
    machine.wait_until_fails(NOVA_PROCESSES, timeout=WAIT)
    machine.sleep(STAYS_OFF_SECS)
    assert unit_property("ActiveState") == "inactive", unit_property("ActiveState")
    assert unit_property("NRestarts") == restarts, "systemd restarted novad"
    machine.fail(NOVA_PROCESSES)


start_all()
# The autologin starts the compositor, which starts Nova. Polled, since the
# unit has no pending job to wait on until the compositor is up.
machine.wait_until_succeeds(as_user(f"systemctl --user is-active --quiet {UNIT}"), timeout=SESSION_START)
# The session's first pointer input, so it isn't a test's click.
machine.succeed(pointer_to(*POINTER_REST))
machine.sleep(POINTER_WAKE_SECS)

with subtest("the session starts Nova, shown as Nova and NovaUI"):
    first = wait_running()
    assert process_name(first["pid"]) == "Nova"
    assert process_name(first["ui"]["pid"]) == "NovaUI"
    assert first["ui"]["restarts"] == 0
    machine.wait_until_succeeds(
        as_user("nova logs --no-follow --ui | grep -q 'nova: connected to novad'"), timeout=WAIT
    )
