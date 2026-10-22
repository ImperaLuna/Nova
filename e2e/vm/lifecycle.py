# Nova's lifecycle under systemd and a real compositor, driven through the
# `nova` command the way a user would. default.nix defines USER, RUNTIME_DIR
# and QUIT_COMPOSITOR above this script.
import datetime as dt
import json
import shlex

UNIT = "nova.service"
NOVA_PROCESSES = "pgrep -x 'Nova|NovaUI'"
WAIT = dt.timedelta(minutes=2)
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


def wait_gone(pid: int) -> None:
    machine.wait_until_fails(f"test -e /proc/{pid}", timeout=WAIT)


def assert_stays_off(restarts: str) -> None:
    machine.wait_until_fails(NOVA_PROCESSES, timeout=WAIT)
    machine.sleep(STAYS_OFF_SECS)
    assert unit_property("ActiveState") == "inactive", unit_property("ActiveState")
    assert unit_property("NRestarts") == restarts, "systemd restarted novad"
    machine.fail(NOVA_PROCESSES)


start_all()
machine.wait_for_unit(UNIT, user=USER)

with subtest("the session starts Nova, shown as Nova and NovaUI"):
    first = wait_running()
    assert process_name(first["pid"]) == "Nova"
    assert process_name(first["ui"]["pid"]) == "NovaUI"
    assert first["ui"]["restarts"] == 0
    machine.wait_until_succeeds(
        as_user("nova logs --no-follow --ui | grep -q 'nova: connected to novad'"), timeout=WAIT
    )

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
    machine.succeed(as_user(QUIT_COMPOSITOR))
    assert_stays_off(restarts)
    assert unit_property("Result") == "success", unit_property("Result")
