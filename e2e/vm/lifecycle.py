# Nova's lifecycle under systemd and a real compositor, driven through the
# `nova` command the way a user would, and a screenshot of each UI state for
# compare.sh. default.nix defines USER, UID and QUIT_COMPOSITOR above this
# script.
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
# Where the pointer waits between clicks, away from the bar.
POINTER_REST = (640, 700)
# How long the screen must stay the same before a screenshot.
SETTLED_SECS = 5
# How long to watch that a stopped unit stays stopped: past its RestartSec.
STAYS_OFF_SECS = 3


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


def click(x: int, y: int) -> None:
    """Clicks at (x, y) on the screen with ydotool's virtual mouse, then
    moves the pointer out of the way so no hover shows in screenshots.
    Both compositors run it with flat acceleration, so a move is 1:1."""
    ydotool = "YDOTOOL_SOCKET=/run/ydotoold/socket ydotool"
    rest_x, rest_y = POINTER_REST
    machine.succeed(
        f"{ydotool} mousemove --absolute -x {x} -y {y} && {ydotool} click 0xC0 && "
        f"{ydotool} mousemove --absolute -x {rest_x} -y {rest_y}"
    )


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
    click(*AI_USAGE_PILL)
    screenshot("panel-open")
    # The same spot closes it again.
    click(*AI_USAGE_PILL)

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
