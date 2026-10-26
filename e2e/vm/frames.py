# The second half of the VM test, after lifecycle.py, in a VM of its own
# beside session.py: what the UI draws while idle and while a panel opens or
# closes, the layers it moves between over a fullscreen window, and the
# panels off the stage. Takes no screenshots.

with subtest("an idle bar draws nothing, shown or beside a fullscreen window; the panel draws briefly; the launcher over a fullscreen window moves Nova to Overlay and back, each time; the bar gives a fullscreen window its space, and its pills take clicks once back"):
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
    given = ui_log_count(BAR_SPACE_GIVEN)
    machine.succeed(as_user(FULLSCREEN_WINDOW))
    retry(lambda _last_try: ui_log_count(BAR_SPACE_GIVEN) > given, timeout=WAIT)
    assert_idle("beside a fullscreen window")
    layers_follow_the_launcher(LAYER_ROUNDS)
    kept = ui_log_count(BAR_SPACE_KEPT)
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    retry(lambda _last_try: ui_log_count(BAR_SPACE_KEPT) > kept, timeout=WAIT)
    # The pills slid back in with their shelf, and take clicks where they are
    # now: they used to keep taking them where they had slid out of sight
    # (IMP-118).
    click_until_logged(*AI_USAGE_PILL, AI_USAGE_OPENED)
    press_until_logged(ESC, AI_USAGE_CLOSED)
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
    machine.succeed(as_user(CLOSE_WINDOW))
    machine.wait_until_fails(as_user(NOVA_SEES_FOOT), timeout=WAIT)
    machine.systemctl("unset-environment NOVA_DEMO_PANELS WAYLAND_DEBUG NO_COLOR NOVA_LOG", user=USER)
    machine.succeed(as_user("nova restart"))
    first = wait_running(not_daemon=demo["pid"])
