# The first half of the VM test, after lifecycle.py: screenshots of each UI
# state, the panels, lock screen, power menu and launcher, the volume capsule,
# and Nova's lifecycle under systemd. frames.py runs beside it in a VM of its
# own.

with subtest("login apps start once Nova's tray is up"):
    # systemd's autostart unit for it waits for nova.service to be ready
    # (Type=notify, app-.service.d/nova.conf), when the tray's name is owned.
    machine.wait_until_succeeds(as_user(f"test -s {RUNTIME_DIR}/nova-login-probe"), timeout=WAIT)
    probe = machine.succeed(as_user(f"cat {RUNTIME_DIR}/nova-login-probe")).strip()
    assert probe == "b true", f"the login app started before the tray: {probe}"

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

with subtest("a click outside or Esc closes the AI usage panel"):
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED)
    click_until_logged(*POINTER_REST, AI_USAGE_CLOSED)
    # It takes no typing, but holds the keyboard while open so Esc reaches
    # it, also when a keybind opened it.
    before = ui_log_count(AI_USAGE_OPENED)
    machine.succeed(as_user("nova toggle ai-usage"))
    retry(lambda _last_try: ui_log_count(AI_USAGE_OPENED) > before, timeout=CLICK_EFFECT)
    press_until_logged(ESC, AI_USAGE_CLOSED)

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

with subtest("nova toggle power opens the power menu; Esc closes it and its L key locks"):
    for keys, done in ((ESC, POWER_CLOSED), (KEY_L, SESSION_LOCKED)):
        before = ui_log_count(POWER_OPENED)
        machine.succeed(as_user("nova toggle power"))
        retry(lambda _last_try: ui_log_count(POWER_OPENED) > before, timeout=CLICK_EFFECT)
        press_until_logged(keys, done)
    machine.succeed(f"{YDOTOOL} type {shlex.quote(PASSWORD)}")
    press_until_logged(ENTER, SESSION_UNLOCKED)

with subtest("nova toggle launcher opens the launcher, empty; typing finds foot, Enter starts it in a scope of its own, Esc closes"):
    before = ui_log_count(LAUNCHER_TYPING)
    machine.succeed(as_user("nova toggle launcher"))
    retry(lambda _last_try: ui_log_count(LAUNCHER_TYPING) > before, timeout=CLICK_EFFECT)
    screenshot("launcher-open")
    machine.succeed(f"{YDOTOOL} type {FOOT}")
    screenshot("launcher")
    try:
        press_until_logged(ENTER, LAUNCHER_CLOSED)
    except Exception:
        # Whether novad found foot and what starting it said.
        for command in ("nova logs --no-follow -n 200 | grep -i launch", "nova logs --no-follow --ui -n 50"):
            print(f"$ {command}")
            print(machine.execute(as_user(command))[1])
        raise
    machine.wait_until_succeeds(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    # Not in nova.service: restarting Nova leaves the app running.
    foot_running = f"systemctl --user list-units --type=service --state=running --no-legend | grep -qF {FOOT_UNIT}"
    machine.succeed(as_user(foot_running))
    # Over foot fullscreen: the compositor draws a focused fullscreen window
    # over the Top layer, so while it's open Nova moves to Overlay, and back
    # once it's closed. The stage stays there while the window is
    # fullscreen, so the pills can peek over it (IMP-182).
    given = ui_log_count(BAR_SPACE_GIVEN)
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    retry(lambda _last_try: ui_log_count(BAR_SPACE_GIVEN) > given, timeout=WAIT)
    before = ui_log_count(LAUNCHER_TYPING)
    machine.succeed(as_user("nova toggle launcher"))
    try:
        # Opened over it, it holds the keyboard too, so Esc reaches it. On
        # Hyprland it twice never did (IMP-177): who had the keyboard then.
        retry(lambda _last_try: ui_log_count(LAUNCHER_TYPING) > before, timeout=CLICK_EFFECT)
        assert_layers("overlay", "nova-panel", "nova-stage", "nova-click-catcher")
        press_until_logged(ESC, LAUNCHER_CLOSED)
    except Exception:
        for command in (LIST_LAYERS, LIST_WINDOWS, "nova logs --no-follow --ui -n 50", COMPOSITOR_LOG):
            print(f"$ {command}")
            print(machine.execute(as_user(command))[1])
        raise
    assert_layers("top", "nova-click-catcher")
    assert_layers("overlay", "nova-stage")
    # The top edge brings the pills back over the fullscreen window, and
    # they take a click. On Hyprland they stayed under it once the window
    # had gone fullscreen after the stage mapped (IMP-182).
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED, clicker=peek_and_click)
    press_until_logged(ESC, AI_USAGE_CLOSED)
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    assert_layers("top", "nova-stage")

with subtest("logind's lock-session locks the screen, and novad holds back sleep until it can lock"):
    # Suspending the VM would stop the test, so only the inhibitor that
    # makes a suspend wait is checked here.
    machine.wait_until_succeeds(f"systemd-inhibit --list --no-pager | grep {shlex.quote(NOVA_INHIBITOR)}", timeout=WAIT)
    before = ui_log_count(SESSION_LOCKED)
    machine.succeed("loginctl lock-sessions")
    retry(lambda _last_try: ui_log_count(SESSION_LOCKED) > before, timeout=CLICK_EFFECT)
    machine.succeed(f"{YDOTOOL} type {shlex.quote(PASSWORD)}")
    press_until_logged(ENTER, SESSION_UNLOCKED)

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

    def volume_change_shows(target: str) -> int:
        """Changes the volume and waits for the capsule; returns how many
        times it had hidden before the change, for hides()."""
        assert volume() != target, f"the volume is already {target}, so setting it changes nothing"
        before, hidden = ui_log_count(VOLUME_SHOWN), ui_log_count(VOLUME_HIDDEN)
        machine.succeed(as_user(WPCTL_SINK.format("set-volume") + f" {target}"))
        try:
            retry(lambda _last_try: ui_log_count(VOLUME_SHOWN) > before, timeout=CLICK_EFFECT)
        except Exception:
            print("the capsule didn't come out; wpctl and the UI's log:")
            ui_log = f"nova logs --no-follow --ui -n {LOG_HISTORY} | grep -iE 'nova:|pipewire|volume|warn|error' | tail -n 60"
            for command in ("wpctl status", WPCTL_SINK.format("get-volume"), WPCTL_SINK.format("inspect"), ui_log):
                print(machine.execute(as_user(command))[1])
            raise
        return hidden

    # Counted from before the change: the capsule hides
    # Tokens.durationCapsuleHold after it, which a slow log read after it
    # can already include.
    def hides(hidden: int) -> None:
        retry(lambda _last_try: ui_log_count(VOLUME_HIDDEN) > hidden, timeout=CLICK_EFFECT)

    hides(volume_change_shows("0.30" if volume() != "0.30" else "0.35"))
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
    hides(hidden)
    # On the overlay layer, so it shows over a fullscreen window.
    open_foot("foot-volume")
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    hides(volume_change_shows("0.60"))
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)

with subtest("`nova volume` steps and mutes, and shows the capsule even at 100%"):

    def nova_volume_shows(key: str) -> None:
        shown, hidden = ui_log_count(VOLUME_SHOWN), ui_log_count(VOLUME_HIDDEN)
        out = machine.succeed(as_user(f"nova volume {key}"))
        assert out == "", f"`nova volume {key}` printed {out!r}; a keybind should stay quiet"
        retry(lambda _last_try: ui_log_count(VOLUME_SHOWN) > shown, timeout=CLICK_EFFECT)
        hides(hidden)

    hides(volume_change_shows("1.00"))
    # Nothing changes at the top, and it still comes out.
    nova_volume_shows("up")
    assert volume() == "1.00", f"`nova volume up` went past 100%: {volume()}"
    nova_volume_shows("down")
    assert volume() == "0.95", f"`nova volume down` from 100% gave {volume()}"
    nova_volume_shows("mute")
    assert "MUTED" in machine.succeed(as_user(WPCTL_SINK.format("get-volume"))), "`nova volume mute` didn't mute"
    nova_volume_shows("mute")
    assert "MUTED" not in machine.succeed(as_user(WPCTL_SINK.format("get-volume"))), "a second mute didn't unmute"

with subtest("the capsule stays in while quick settings shows the volume, and slides away as it opens"):
    qs_shown, qs_hidden = ui_log_count(VOLUME_SHOWN), ui_log_count(VOLUME_HIDDEN)
    machine.succeed(as_user("nova volume up"))
    retry(lambda _last_try: ui_log_count(VOLUME_SHOWN) > qs_shown, timeout=CLICK_EFFECT)
    qs_opened = ui_log_count(QUICK_SETTINGS_OPENED)
    machine.succeed(as_user("nova toggle quick-settings"))
    retry(lambda _last_try: ui_log_count(QUICK_SETTINGS_OPENED) > qs_opened, timeout=CLICK_EFFECT)
    # Well before the capsule's own hold runs out.
    retry(lambda _last_try: ui_log_count(VOLUME_HIDDEN) > qs_hidden, timeout=dt.timedelta(seconds=CAPSULE_HOLD_SECS - 1))
    qs_shown = ui_log_count(VOLUME_SHOWN)
    qs_volume = volume()
    machine.succeed(as_user("nova volume down"))
    retry(lambda _last_try: volume() != qs_volume, timeout=CLICK_EFFECT)
    machine.sleep(NO_EFFECT_SECS)
    assert ui_log_count(VOLUME_SHOWN) == qs_shown, "the capsule came out over open quick settings"
    qs_closed = ui_log_count(QUICK_SETTINGS_CLOSED)
    machine.succeed(as_user("nova toggle quick-settings"))
    retry(lambda _last_try: ui_log_count(QUICK_SETTINGS_CLOSED) > qs_closed, timeout=CLICK_EFFECT)

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

for add_output, focus_first, remove_output in ADDED_OUTPUTS:
    with subtest("a click on another screen closes quick settings"):
        # While quick settings holds the keyboard, Hyprland gives it the
        # clicks on every screen, which it used to drop (IMP-178).
        bars = ui_log_count(BAR_SHOWN)
        machine.succeed(as_user(add_output))
        try:
            retry(lambda _last_try: ui_log_count(BAR_SHOWN) > bars, timeout=WAIT)

            # novad knows the new output, and that the first has the focus, so
            # `nova toggle` opens quick settings there. Hyprland focuses an
            # output it adds, which could come after a focus asked for early.
            def first_focused(last_try: bool) -> bool:
                machine.succeed(as_user(focus_first))
                outputs = ((status() or {}).get("desktop") or {}).get("outputs", [])
                if last_try:
                    print(f"novad's outputs: {outputs}")
                focused = [output["name"] for output in outputs if output.get("focused")]
                return ADDED_OUTPUT in [output["name"] for output in outputs] and focused not in ([], [ADDED_OUTPUT])

            retry(first_focused, timeout=CLICK_EFFECT)
            # On the first output: a panel surface
            # there takes more than its parked 1x1, and none on the added one.
            opened = ui_log_count(QUICK_SETTINGS_OPENED)
            machine.succeed(as_user("nova toggle quick-settings"))
            retry(lambda _last_try: ui_log_count(QUICK_SETTINGS_OPENED) > opened, timeout=CLICK_EFFECT)

            def opened_on_the_first(last_try: bool) -> bool:
                shown = {
                    name: any(s["namespace"] == "nova-panel" and s["w"] > 1 for level in monitor["levels"].values() for s in level)
                    for name, monitor in json.loads(machine.succeed(as_user(LIST_LAYERS))).items()
                }
                if last_try:
                    print(f"panel surfaces shown, by output: {shown}")
                return not shown.pop(ADDED_OUTPUT) and any(shown.values())

            retry(opened_on_the_first, timeout=CLICK_EFFECT)
            click_until_logged(*ON_ADDED_OUTPUT, QUICK_SETTINGS_CLOSED)
        finally:
            machine.succeed(as_user(remove_output))

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
