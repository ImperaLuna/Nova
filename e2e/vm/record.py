# Records what a real compositor tells novad, for the compositor tests in
# framework/compositor: the raw event stream while a script of
# workspace, window and layout changes runs, and the compositor's own view of
# its state after each step. default.nix defines USER, UID, IPC_ENV and the
# compositor's commands (STEPS, QUERIES, STREAM, COMPOSITOR_LOG) above this
# script.
#
# Build `.#checks.<system>.e2e-<compositor>.record` and copy its output into
# framework/compositor/recorded/<compositor>; `just record` does both.
import datetime as dt
import json
import shlex

RUNTIME_DIR = f"/run/user/{UID}"
SESSION_START = dt.timedelta(minutes=10)
OUT = "/tmp/recorded"
# A step is done once the event stream has been quiet this many seconds
# (and has what the step waits for, if anything).
QUIET_SECS = 3
WAIT = dt.timedelta(minutes=3)
# The second window reads these fifos and changes its title or rings the
# bell when they're written to, so each happens exactly when a step says.
RETITLE_FIFO = "/tmp/retitle"
BELL_FIFO = "/tmp/bell"
RENAMED = "Renamed, with, commas"


def as_user(command: str) -> str:
    env = (
        f"export XDG_RUNTIME_DIR={RUNTIME_DIR}; "
        f"export WAYLAND_DISPLAY=$(cd {RUNTIME_DIR} && ls wayland-* | grep -v lock | head -n1); "
        f"export {IPC_ENV}; "
    )
    return f"su -l {USER} -c {shlex.quote(env + command)}"


def window(app_id: str, title: str, script: str = "exec sleep infinity") -> str:
    """A foot terminal with this app id and title, running `script`."""
    return (
        f"setsid foot -o bell.urgent=yes -o bell.notify=no --app-id={app_id} --title={shlex.quote(title)} "
        f"sh -c {shlex.quote(script)} >/dev/null 2>&1 &"
    )


def snapshot(step: int) -> None:
    """The compositor's answer to each query, in <step>/<query>.json."""
    machine.succeed(as_user(f"mkdir -p {OUT}/{step:02}"))
    for name, command in QUERIES.items():
        # Retried: under emulation a busy compositor can miss its IPC timeout.
        machine.wait_until_succeeds(as_user(f"{command} > {OUT}/{step:02}/{name}.json"), timeout=WAIT)


# The second window retitles itself (to a title with commas, which
# Hyprland's events don't escape), then rings an urgent bell.
SECOND_WINDOW = (
    f"read _ < {RETITLE_FIFO}; printf '\\033]2;%s\\007' {shlex.quote(RENAMED)}; "
    f"read _ < {BELL_FIFO}; printf '\\a'; exec sleep infinity"
)
# The steps every compositor does the same way, and what each waits to see
# in the events: under emulation a window can take a while to show.
COMMON_STEPS = {
    "open first window": (window("nova-first", "First window"), "nova-first"),
    "open second window": (window("nova-second", "Second window", SECOND_WINDOW), "nova-second"),
    "retitle the second window": (f"echo > {RETITLE_FIFO}", RENAMED),
    "ring the second window's bell": (f"echo > {BELL_FIFO}", None),
}


# Steps each compositor does its own way, and what to wait for in the
# events (any of them): under emulation a fullscreen window's new size, or
# its closing, can come after the events have been quiet for QUIET_SECS.
STEP_MARKERS = {
    "fullscreen the focused window": ("WindowLayoutsChanged", "fullscreen>>1"),
    "close the focused window": ("WindowClosed", "closewindow>>"),
    "hide a window in the special workspace": ("movewindowv2>>",),
    "show the special workspace": ("activespecial>>special:magic",),
    "hide the special workspace": ("activespecial>>,",),
}


def events_bytes() -> int:
    return int(machine.succeed(f"stat -c %s {OUT}/events.txt"))


def settle(markers: tuple[str, ...]) -> None:
    if markers:
        patterns = " ".join(f"-e {shlex.quote(marker)}" for marker in markers)
        machine.wait_until_succeeds(f"grep -qF {patterns} {OUT}/events.txt", timeout=WAIT)
    quiet = [0, -1]

    def is_quiet(_last_try: bool) -> bool:
        size = events_bytes()
        quiet[0] = quiet[0] + 1 if size == quiet[1] else 0
        quiet[1] = size
        return quiet[0] >= QUIET_SECS

    # retry() calls this once a second.
    retry(is_quiet, timeout=WAIT)


def run(step: str) -> None:
    command, marker = COMMON_STEPS.get(step) or (STEPS[step], None)
    markers = (marker,) if marker else STEP_MARKERS.get(step, ())
    print(f"step: {step}: {command}")
    machine.succeed(as_user(command))
    settle(markers)


start_all()
# Up once it answers its IPC.
machine.wait_until_succeeds(as_user(next(iter(QUERIES.values()))), timeout=SESSION_START)
machine.succeed(f"mkdir -p {OUT} && chown {USER} {OUT}")
machine.succeed(as_user(f"mkfifo {RETITLE_FIFO} {BELL_FIFO}"))
# The stream stays open (socat waits on its stdin) until the test ends.
stream = shlex.quote(f"{STREAM} > {OUT}/events.txt")
machine.succeed(as_user(f"setsid sh -c {stream} >/dev/null 2>&1 &"))
machine.wait_until_succeeds(f"test -e {OUT}/events.txt", timeout=WAIT)
settle(())

# steps.json: each step's name and how much of events.txt had arrived once
# it settled; <step>/ has the queries' answers at that point. Step 0 is the
# state when the stream started.
steps = [{"name": "start", "events_bytes": events_bytes()}]
snapshot(0)
# Each compositor's STEPS say how to do the steps that aren't the same
# everywhere; a step it has no command for is skipped.
try:
    for step in [
        "open first window",
        "focus the next workspace",
        "open second window",
        "retitle the second window",
        "switch the keyboard layout",
        "name the workspace",
        "focus the previous workspace",
        "ring the second window's bell",
        "focus the other output",
        "focus the first output",
        "fullscreen the focused window",
        "close the focused window",
        "hide a window in the special workspace",
        "show the special workspace",
        "hide the special workspace",
    ]:
        if step in STEPS or step in COMMON_STEPS:
            run(step)
            steps.append({"name": step, "events_bytes": events_bytes()})
            snapshot(len(steps) - 1)

except Exception:
    # What the compositor said about it, next to the failure.
    print(machine.execute(as_user(COMPOSITOR_LOG))[1])
    raise

machine.succeed(f"echo {shlex.quote(json.dumps(steps, indent=2))} > {OUT}/steps.json")
machine.copy_from_machine(OUT)
