# Nova's lifecycle under systemd and a real compositor, driven through the
# `nova` command the way a user would, and a screenshot of each UI state for
# compare.sh. default.nix defines USER, UID, PASSWORD, COMPOSITOR, QUIT_COMPOSITOR,
# FOCUS_NEXT_WORKSPACE, FULLSCREEN_WINDOW, CLOSE_WINDOW, LIST_WINDOWS,
# LIST_LAYERS, COMPOSITOR_LOG and FRAMES_REPORT above this script.
import datetime as dt
import itertools
import json
import math
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
# top-left corner and is Tokens.barHeight (36) tall.
AI_USAGE_PILL = (24, 26)
# What the pill logs when its panel opens or closes (ui/ai_usage/AiUsage.qml).
AI_USAGE_OPENED = "nova: ai usage panel opened"
AI_USAGE_CLOSED = "nova: ai usage panel closed"
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
SETTLED_SECS = 5
# How long to watch that a stopped unit stays stopped: past its RestartSec.
STAYS_OFF_SECS = 3
# How long an idle UI must commit nothing to its surfaces. Long enough for
# niri's once-a-second callbacks to a covered surface (IMP-38).
IDLE_SECS = 10
# Lines of `nova logs` history that reach back to the start of a run.
LOG_HISTORY = 1_000_000
# A frame the UI hands the compositor, in its WAYLAND_DEBUG lines.
SURFACE_COMMIT = r"wl_surface#[0-9]*\.commit("
# The same line, with libwayland's timestamp and the surface, after
# `nova logs`'s own time, level and source. libwayland 1.26 stamps the wall
# clock as [HH:MM:SS.micros]; older ones stamp milliseconds as [ms.frac].
TIMED_COMMIT = re.compile(r"\[\s*(?:(\d+):(\d+):)?(\d+(?:\.\d+)?)\].*-> wl_surface#(\d+)\.commit\(")
# Qt's own timing of each frame it renders (qt.scenegraph.time.renderloop):
# sync is Nova's QML handing the scene over, render the (software) GPU work.
QT_FRAME = re.compile(r"syncAndRender: frame rendered in (\d+)ms.*\bsync=(\d+), render=(\d+)")
# How long the panel may go on drawing after its pill logs that it opened or
# closed. The longest animation, the meters filling, ends at
# Tokens.durationPanel * 0.6 + Tokens.durationFill (750 ms) after opening;
# the rest is room for software rendering's late last frame.
MORPH_DRAWS_FOR = dt.timedelta(milliseconds=1200)
# The VM's outputs refresh at 60 Hz. A surface never needs more than one
# frame per refresh.
REFRESH_HZ = 60
# Frames a surface may draw per refresh before the check fails. Software
# rendering lets some come early (niri's VM closed the panel with frames
# 14.8 ms apart on average), so one and a half, which drawing every frame
# twice still exceeds.
FRAMES_PER_REFRESH = 1.5
# How many surfaces may draw while the panel opens or closes: the stage on
# the pill's screen alone. The bar's exclusion zone used to draw a frame
# too, each time an animation started (IMP-49).
MORPH_SURFACES = 1
# The placeholder panels off the stage (ui/demo/DemoPanels.qml, on with
# NOVA_DEMO_PANELS=1), opened over Quickshell's IPC.
DEMO_PANELS = ("demo-sidebar", "demo-tab", "demo-center")
# What the volume capsule logs as it comes out and once it's gone
# (ui/volume/VolumeOsd.qml).
VOLUME_SHOWN = "nova: volume capsule shown"
VOLUME_HIDDEN = "nova: volume capsule hidden"
# The UI has the default sink's starting volume, so a change from here shows.
VOLUME_FOUND = "nova: volume output found"
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
    # The whole run: with WAYLAND_DEBUG on, a click logs far more than the
    # last 50 lines `nova logs` shows by default.
    out = machine.succeed(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -cF {shlex.quote(line)} || true"))
    return int(out.strip() or 0)


def window_geometry() -> list:
    """Where each window is and how big, as the compositor lists them
    (niri's `layout`, Hyprland's `at` and `size`), and nothing else: a panel
    that takes typing moves the focus, but must move no window."""
    windows = json.loads(machine.succeed(as_user(LIST_WINDOWS)))
    return [{key: window[key] for key in GEOMETRY_KEYS if key in window} for window in windows]


def demo_logged(panel: str, state: str) -> str:
    """What a placeholder panel logs when it's "opened" or "closed"."""
    return f"nova: demo panel {panel} {state}"


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
    FRAMES_PER_REFRESH per refresh, all within MORPH_DRAWS_FOR. Prints how
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
    for line in after_logged:
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
    # anyway, so no budget.
    print(machine.succeed(f"{FRAMES_REPORT} -v refresh='*={REFRESH_HZ}' {capture}"))
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
        # Rounded up: timestamps are rounded and frames jitter.
        own = times[-1] - times[0]
        most = math.ceil(own * REFRESH_HZ * FRAMES_PER_REFRESH / 1000) + 1
        assert len(times) <= most, f"wl_surface#{surface} drew {len(times)} frames in {own:.0f} ms after the panel {what}, more than {FRAMES_PER_REFRESH} per refresh ({most})"


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

with subtest("screenshot: the bar"):
    screenshot("bar")
    # The log next to the screenshot, for when the bar isn't on it.
    print(machine.succeed(as_user("nova logs --no-follow -n 100")))

with subtest("screenshot: the AI usage panel open, grown from its pill"):
    machine.wait_until_succeeds(as_user(f"nova logs --no-follow --ui | grep -qF {shlex.quote(AI_USAGE_SHOWN)}"), timeout=WAIT)
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED)
    screenshot("panel-open")
    # The same spot closes it again.
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_CLOSED)

with subtest("nova toggle opens and closes the AI usage panel, as a keybind would"):
    for line in (AI_USAGE_OPENED, AI_USAGE_CLOSED):
        before = ui_log_count(line)
        machine.succeed(as_user("nova toggle ai-usage"))
        retry(lambda _last_try: ui_log_count(line) > before, timeout=CLICK_EFFECT)

with subtest("nova lock covers the screens until the user's password checks out"):
    before = ui_log_count(SESSION_LOCKED)
    machine.succeed(as_user("nova lock"))
    retry(lambda _last_try: ui_log_count(SESSION_LOCKED) > before, timeout=CLICK_EFFECT)
    machine.succeed(f"{YDOTOOL} type {shlex.quote('not-' + PASSWORD)}")
    press_until_logged(ENTER, WRONG_PASSWORD)
    assert ui_log_count(SESSION_UNLOCKED) == 0, "a wrong password unlocked"
    machine.succeed(f"{YDOTOOL} type {shlex.quote(PASSWORD)}")
    press_until_logged(ENTER, SESSION_UNLOCKED)

with subtest("an idle bar draws nothing, shown or under a fullscreen window; the panel draws briefly"):
    # novad hands its environment to the UI.
    # Without libwayland's colors, which the timestamps would hide behind.
    # Qt's timing of each frame, for morph_frames to print. Qt logs it at
    # debug level, which novad keeps only with NOVA_LOG=debug.
    machine.systemctl(
        "set-environment WAYLAND_DEBUG=client NO_COLOR=1 QT_LOGGING_RULES=qt.scenegraph.time.renderloop.debug=true NOVA_LOG=debug NOVA_FRAME_LOG=1",
        user=USER,
    )
    machine.succeed(as_user("nova restart"))
    watched = wait_running(not_daemon=first["pid"])
    machine.wait_until_succeeds(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -qF {shlex.quote(AI_USAGE_SHOWN)}"), timeout=WAIT)
    assert_idle("with the bar shown")
    # Opening and closing draws (so the count sees frames when there are
    # some), on few surfaces, briefly, and then stops.
    morph_frames("opened", AI_USAGE_OPENED)
    morph_frames("closed", AI_USAGE_CLOSED)
    # Covered, as under a video or a game. On the desktop, niri then sent the
    # bar a frame callback a second and Qt redrew it each time (IMP-38); niri
    # in this VM sends none, so the headless e2e guards that fix instead.
    open_foot("foot-fullscreen")
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    assert_idle("under a fullscreen window")
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    machine.systemctl("unset-environment WAYLAND_DEBUG NO_COLOR QT_LOGGING_RULES NOVA_LOG NOVA_FRAME_LOG", user=USER)
    machine.succeed(as_user("nova restart"))
    # The run the rest of the test knows.
    first = wait_running(not_daemon=watched["pid"])

with subtest("panels off the stage open and close without moving windows"):
    # With libwayland's lines (no colors), so a failure shows where the
    # compositor sent the pointer; novad keeps them only with NOVA_LOG=debug.
    machine.systemctl("set-environment NOVA_DEMO_PANELS=1 WAYLAND_DEBUG=client NO_COLOR=1 NOVA_LOG=debug", user=USER)
    machine.succeed(as_user("nova restart"))
    demo = wait_running(not_daemon=first["pid"])
    machine.wait_until_succeeds(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -qF {shlex.quote(BAR_SHOWN)}"), timeout=WAIT)
    toggle_demo = f"quickshell ipc --pid {demo['ui']['pid']} call demo toggle"
    open_foot("foot-demo")
    windows = window_geometry()
    assert windows and all(windows), f"no window geometry in {LIST_WINDOWS}"
    for panel in DEMO_PANELS:
        for state in ("opened", "closed"):
            before = ui_log_count(demo_logged(panel, state))
            machine.succeed(as_user(f"{toggle_demo} {panel}"))
            retry(lambda _last_try: ui_log_count(demo_logged(panel, state)) > before, timeout=CLICK_EFFECT)
            assert window_geometry() == windows, f"windows moved with {panel} {state}"
    # The center one takes typing, like the launcher will: a click on it
    # keeps it open, one anywhere else (the click catcher) or Esc closes it.
    center = "demo-center"
    for close in ("click outside", "Esc"):
        before = ui_log_count(demo_logged(center, "opened"))
        machine.succeed(as_user(f"{toggle_demo} {center}"))
        retry(lambda _last_try: ui_log_count(demo_logged(center, "opened")) > before, timeout=CLICK_EFFECT)
        closed = demo_logged(center, "closed")
        before = ui_log_count(closed)
        click(*DEMO_CENTER_INSIDE)
        machine.sleep(NO_EFFECT_SECS)
        assert ui_log_count(closed) == before, "a click on the panel closed it"
        try:
            if close == "Esc":
                press_until_logged(ESC, closed)
            else:
                click_until_logged(*POINTER_REST, closed)
        except Exception:
            print(f"{close} didn't close {center}; the layer surfaces, the UI's log and the compositor's:")
            pointer = "get_layer_surface|set_anchor|wl_pointer|wl_keyboard#[0-9]+\\.(enter|leave)"
            ui_log = f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -E 'nova:|{pointer}' | tail -n 150"
            for command in (LIST_LAYERS, ui_log, COMPOSITOR_LOG):
                print(machine.execute(as_user(command))[1])
            raise
    # AI usage takes no typing: it closes as the focus moves on to a window
    # (StagePanel closesOnFocusLoss), and the click reaches that window. Its
    # surface takes the focus on the click that opens it, which the pointer
    # coming over the pill first offers.
    machine.succeed(pointer_to(*AI_USAGE_PILL))
    machine.sleep(1)
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED)
    try:
        click_until_logged(*POINTER_REST, AI_USAGE_CLOSED)
    except Exception:
        print("a click on the window didn't close AI usage; the UI's log and the compositor's:")
        focus = "nova:|wl_keyboard#[0-9]+\\.(enter|leave)|set_keyboard_interactivity"
        for command in (f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -E '{focus}' | tail -n 80", COMPOSITOR_LOG):
            print(machine.execute(as_user(command))[1])
        raise
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    machine.systemctl("unset-environment NOVA_DEMO_PANELS WAYLAND_DEBUG NO_COLOR NOVA_LOG", user=USER)
    machine.succeed(as_user("nova restart"))
    first = wait_running(not_daemon=demo["pid"])

with subtest("a volume change slides the capsule out and back, over fullscreen too; scrolling on it steps the volume"):
    # The volume Nova found at startup is where it starts, not a change. The
    # UI may have just restarted, so a change before it found the sink would
    # be its starting volume.
    def found_since_ui_start(_last_try: bool) -> bool:
        log = machine.succeed(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY}"))
        return VOLUME_FOUND in log.rpartition(UI_LAUNCHED)[2]

    retry(found_since_ui_start, timeout=CLICK_EFFECT)
    assert ui_log_count(VOLUME_SHOWN) == 0, "the capsule came out without a volume change"

    def volume() -> str:
        """The default sink's volume as wpctl prints it, like "0.40"."""
        out = machine.succeed(as_user(WPCTL_SINK.format("get-volume")))
        return out.split()[1]

    def volume_change_shows(target: str) -> None:
        assert volume() != target, f"the volume is already {target}, so setting it changes nothing"
        before = ui_log_count(VOLUME_SHOWN)
        machine.succeed(as_user(WPCTL_SINK.format("set-volume") + f" {target}"))
        try:
            retry(lambda _last_try: ui_log_count(VOLUME_SHOWN) > before, timeout=CLICK_EFFECT)
        except Exception:
            print("the capsule didn't come out; wpctl and the UI's log:")
            ui_log = f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -iE 'nova:|pipewire|volume|warn|error' | tail -n 60"
            for command in ("wpctl status", WPCTL_SINK.format("get-volume"), WPCTL_SINK.format("inspect"), ui_log):
                print(machine.execute(as_user(command))[1])
            raise

    def hides() -> None:
        hidden = ui_log_count(VOLUME_HIDDEN)
        retry(lambda _last_try: ui_log_count(VOLUME_HIDDEN) > hidden, timeout=CLICK_EFFECT)

    volume_change_shows("0.30" if volume() != "0.30" else "0.35")
    hides()
    # The pointer on it holds it out, and a wheel notch steps the volume by
    # Tokens.volumeStep, which way depending on how ydotool's wheel maps.
    # The pointer goes there once it has slid out (the compositor picks the
    # surface under the pointer when the pointer moves), in the same command
    # as the change so it's there well within the hold. Retried like clicks,
    # as niri can drop the odd pointer input.
    for attempt in range(CLICK_TRIES):
        target = ("0.50", "0.40")[attempt % 2]
        machine.succeed(pointer_to(*POINTER_REST))
        assert volume() != target, f"the volume is already {target}"
        shown, hidden = ui_log_count(VOLUME_SHOWN), ui_log_count(VOLUME_HIDDEN)
        machine.succeed(
            as_user(WPCTL_SINK.format("set-volume") + f" {target}")
            + f" && sleep {CAPSULE_SLIDE_SECS} && {pointer_to(*CAPSULE)} && {YDOTOOL} mousemove --wheel -x 0 -y 1"
        )
        retry(lambda _last_try: ui_log_count(VOLUME_SHOWN) > shown, timeout=CLICK_EFFECT)
        try:
            retry(lambda _last_try: volume() != target, timeout=CLICK_EFFECT)
            break
        except Exception:
            print(f"the volume is still {target} after a wheel notch on the capsule, scrolling again")
    else:
        print(machine.execute(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -F 'nova:' | tail -n 20"))[1])
        raise AssertionError(f"{CLICK_TRIES} wheel notches on the capsule never changed the volume")
    step = abs(float(volume()) - float(target))
    assert abs(step - 0.05) < 0.001, f"one notch moved the volume from {target} to {volume()}"
    machine.sleep(CAPSULE_HOLD_SECS)
    assert ui_log_count(VOLUME_HIDDEN) == hidden, "the capsule went away under the pointer"
    machine.succeed(pointer_to(*POINTER_REST))
    hides()
    # On the overlay layer, so it shows over a fullscreen window.
    open_foot("foot-volume")
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    volume_change_shows("0.60")
    hides()
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)

with subtest("a workspace switch reaches the UI"):
    running = wait_running()
    desktop = running.get("desktop")
    assert desktop and desktop["compositor"].lower() == COMPOSITOR, desktop

    def ui_focused_workspace(expected: str) -> None:
        ask = f"quickshell ipc --pid {running['ui']['pid']} prop get compositor focusedWorkspace"
        try:
            machine.wait_until_succeeds(as_user(f"test \"$({ask})\" = {expected}"), timeout=WAIT)
        except Exception:
            print(machine.execute(as_user(f"nova status; echo UI: $({ask})"))[1])
            raise

    ui_focused_workspace("1")
    # niri only has a next workspace once this one holds a window.
    open_foot("foot-workspace")
    machine.succeed(as_user(FOCUS_NEXT_WORKSPACE))
    ui_focused_workspace("2")

with subtest("novad restarts a killed UI and saves the lines before it"):
    machine.succeed(f"kill -9 {first['ui']['pid']}")
    crashed = wait_running(not_ui=first["ui"]["pid"])
    assert crashed["pid"] == first["pid"], "novad itself restarted"
    assert crashed["ui"]["restarts"] == 1, crashed["ui"]
    machine.succeed(as_user("nova logs --crash | grep -q 'nova: connected to novad'"))

with subtest("systemd brings back a killed daemon, and the UI with it"):
    restarts = int(unit_property("NRestarts"))
    machine.succeed(f"kill -9 {crashed['pid']}")
    wait_gone(crashed["ui"]["pid"])
    revived = wait_running(not_daemon=crashed["pid"], not_ui=crashed["ui"]["pid"])
    assert int(unit_property("NRestarts")) == restarts + 1

with subtest("nova restart gives new processes; logs --previous reads the run before"):
    machine.succeed(as_user("nova restart"))
    restarted = wait_running(not_daemon=revived["pid"], not_ui=revived["ui"]["pid"])
    old_start = f"starting (pid {revived['pid']})"
    machine.succeed(as_user(f"nova logs --previous -n 1000 | grep -qF {shlex.quote(old_start)}"))

with subtest("SIGTERM stops the daemon for good"):
    restarts = unit_property("NRestarts")
    machine.succeed(f"kill -TERM {restarted['pid']}")
    assert_stays_off(restarts)

with subtest("without systemd, the UI dies with its daemon"):
    # A socket other than the unit's makes `nova` run novad directly.
    socket = f"{RUNTIME_DIR}/nova/direct.sock"
    machine.succeed(as_user(f"nova --socket {socket}"))
    direct = wait_running(socket=socket)
    assert unit_property("ActiveState") == "inactive"
    machine.succeed(f"kill -9 {direct['pid']}")
    wait_gone(direct["ui"]["pid"])

with subtest("nova starts nova.service"):
    machine.succeed(as_user("nova"))
    wait_running()
    assert unit_property("ActiveState") == "active"

with subtest("nova quit stops the unit and leaves nothing running"):
    restarts = unit_property("NRestarts")
    machine.succeed(as_user("nova quit"))
    assert_stays_off(restarts)

with subtest("novad exits cleanly when the compositor goes away"):
    machine.succeed(as_user("nova"))
    wait_running()
    restarts = unit_property("NRestarts")
    # Not succeed: a compositor can exit before it answers the request.
    machine.execute(as_user(QUIT_COMPOSITOR))
    assert_stays_off(restarts)
    assert unit_property("Result") == "success", unit_property("Result")
