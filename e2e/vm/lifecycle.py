# Nova's lifecycle under systemd and a real compositor, driven through the
# `nova` command the way a user would, and a screenshot of each UI state for
# compare.sh. default.nix defines USER, UID, COMPOSITOR, QUIT_COMPOSITOR,
# FOCUS_NEXT_WORKSPACE, FULLSCREEN_WINDOW, CLOSE_WINDOW, LIST_WINDOWS and
# COMPOSITOR_LOG above this script.
import datetime as dt
import json
import shlex

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


def click_until_logged(x: int, y: int, line: str) -> None:
    """Clicks at (x, y) until the UI logs `line` once more than before."""
    before = ui_log_count(line)
    for _ in range(CLICK_TRIES):
        click(x, y)
        try:
            retry(lambda _last_try: ui_log_count(line) > before, timeout=CLICK_EFFECT)
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

with subtest("an idle bar draws nothing, shown or under a fullscreen window"):
    # novad hands its environment to the UI.
    machine.systemctl("set-environment WAYLAND_DEBUG=client", user=USER)
    machine.succeed(as_user("nova restart"))
    watched = wait_running(not_daemon=first["pid"])
    machine.wait_until_succeeds(as_user(f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -qF {shlex.quote(AI_USAGE_SHOWN)}"), timeout=WAIT)
    assert_idle("with the bar shown")
    # The count sees frames when there are some: the panel opening draws.
    commits = f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -c {shlex.quote(SURFACE_COMMIT)} || true"
    before = int(machine.succeed(as_user(commits)).strip() or 0)
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED)
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_CLOSED)
    after = int(machine.succeed(as_user(commits)).strip() or 0)
    assert after > before, f"no surface commits logged while the panel opened and closed ({before} before, {after} after)"
    assert_idle("after the panel closed")
    # Covered, as under a video or a game. On the desktop, niri then sent the
    # bar a frame callback a second and Qt redrew it each time (IMP-38); niri
    # in this VM sends none, so the headless e2e guards that fix instead.
    open_foot("foot-fullscreen")
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    assert_idle("under a fullscreen window")
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    machine.systemctl("unset-environment WAYLAND_DEBUG", user=USER)
    machine.succeed(as_user("nova restart"))
    # The run the rest of the test knows.
    first = wait_running(not_daemon=watched["pid"])

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
