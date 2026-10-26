//! Compositor actions, panels, volume keys, the lock screen and power.

use super::*;

#[test]
fn niri_reaches_subscribers_and_status_and_takes_workspace_actions() {
    let mut fake = None;
    let novad = TestDaemon::with_compositor("niri", |dir| {
        let (compositor, backend) = FakeCompositor::niri(dir);
        fake = Some(compositor);
        backend
    });
    let fake = fake.unwrap();
    let (mut client, desktop) = follow_to_the_end(&novad);
    assert_eq!(desktop["compositor"], "niri");
    let status = client.request(2, "status", None);
    assert_eq!(status["result"]["desktop"], desktop);

    let web = workspace_id(&desktop, "web");
    assert_eq!(client.request(3, "focus_workspace", Some(json!({"id": web})))["result"], Value::Null);
    let output = focused_output(&desktop);
    assert_eq!(
        client.request(4, "step_workspace", Some(json!({"output": output, "step": "next"})))["result"],
        Value::Null
    );
    let expected = |id: i64| json!({"Action": {"FocusWorkspace": {"reference": {"Id": id}}}}).to_string();
    assert_eq!(fake.actions(), [expected(web), expected(web)], "workspace 1 is shown, so next is web");

    let unknown = client.request(5, "focus_workspace", Some(json!({"id": 999})));
    assert!(unknown["error"].as_str().unwrap().contains("no workspace 999"), "{unknown}");
    assert_eq!(fake.actions().len(), 2, "unknown workspaces never reach niri");
}

#[test]
fn hyprland_reaches_subscribers_and_takes_workspace_actions() {
    let mut fake = None;
    let novad = TestDaemon::with_compositor("hyprland", |dir| {
        let (compositor, backend) = FakeCompositor::hyprland(dir);
        fake = Some(compositor);
        backend
    });
    let fake = fake.unwrap();
    let (mut client, desktop) = follow_to_the_end(&novad);
    assert_eq!(desktop["compositor"], "Hyprland");

    let first = workspace_id(&desktop, "1");
    assert_eq!(client.request(2, "focus_workspace", Some(json!({"id": first})))["result"], Value::Null);
    let magic = workspace_id(&desktop, "magic");
    assert_eq!(client.request(3, "focus_workspace", Some(json!({"id": magic})))["result"], Value::Null);
    assert_eq!(fake.actions(), [format!("dispatch workspace {first}"), "dispatch togglespecialworkspace magic".into()]);
}

#[test]
fn hyprland_focuses_an_output_without_its_workspaces() {
    // A special workspace may show there, which focusing by workspace would
    // hide (or, by the one under it, cover the new window with).
    let dir = nova_test_support::TempDir::new("focus-output-hyprland");
    let (fake, backend) = FakeCompositor::hyprland(dir.path());
    let output = nova_protocol::Output { name: "DP-2".into(), focused: false, workspaces: Vec::new() };
    backend.focus_output(&output).unwrap();
    assert_eq!(fake.actions(), ["dispatch focusmonitor DP-2"]);
}

#[test]
fn compositor_actions_without_a_compositor_say_so() {
    let novad = TestDaemon::start("no-compositor", None);
    let mut client = novad.connect();
    let reply = client.request(1, "focus_workspace", Some(json!({"id": 1})));
    assert!(reply["error"].as_str().unwrap().contains("doesn't know this compositor"), "{reply}");
    assert_eq!(client.request(2, "status", None)["result"].get("desktop"), None);
}

#[test]
fn toggle_needs_the_panels_feature() {
    let novad = TestDaemon::start("toggle-off", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["toggle"]})));
    let reply = novad.connect().request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert!(reply["error"].as_str().unwrap().contains("ai-usage feature isn't ready"), "{reply}");
    assert_eq!(ui.request(2, "ping", None)["result"], "pong", "no toggle event without the feature");
}

#[test]
fn toggle_reaches_the_ui_once_it_listens() {
    let novad = TestDaemon::with_ai_usage("toggle", |_| {});
    let mut cli = novad.connect();
    let reply = cli.request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["toggle"]})));
    assert_eq!(cli.request(2, "toggle", Some(json!({"panel": "ai-usage"})))["result"], Value::Null);
    assert_eq!(ui.next_event("toggle"), json!({"panel": "ai-usage"}), "no output without a compositor");
    assert!(cli.request(3, "toggle", Some(json!({"panel": "calculator"})))["error"].is_string());
}

#[test]
fn toggle_names_the_focused_output() {
    let novad = TestDaemon::open_following("toggle-niri", None, |_| {}, |dir| Some(FakeCompositor::niri(dir).1));
    let (mut ui, desktop) = follow_to_the_end(&novad);
    ui.request(2, "subscribe", Some(json!({"topics": ["toggle"]})));
    let reply = novad.connect().request(1, "toggle", Some(json!({"panel": "ai-usage"})));
    assert_eq!(reply["result"], Value::Null);
    let toggle = ui.next_event("toggle");
    assert_eq!(toggle, json!({"panel": "ai-usage", "output": focused_output(&desktop)}));
}

#[test]
fn volume_keys_reach_the_ui_once_it_listens() {
    let novad = TestDaemon::start("volume", None);
    let mut cli = novad.connect();
    let reply = cli.request(1, "volume", Some(json!({"key": "up"})));
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["volume"]})));
    for (id, key) in [(2, "up"), (3, "down"), (4, "mute")] {
        assert_eq!(cli.request(id, "volume", Some(json!({"key": key})))["result"], Value::Null);
        assert_eq!(ui.next_event("volume"), json!({"key": key}));
    }
    assert!(cli.request(5, "volume", Some(json!({"key": "louder"})))["error"].is_string());
}

#[test]
fn lock_reaches_the_ui_and_a_restarted_ui_locks_again() {
    let novad = TestDaemon::start("lock", None);
    let mut cli = novad.connect();
    let reply = cli.request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("UI isn't connected"), "{reply}");

    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(ui.next_event("lock"), json!({"locked": false}), "the state on subscribe");
    // `nova lock` answers once the compositor has covered the screens.
    let locking = std::thread::spawn(move || cli.request(2, "lock", None));
    assert_eq!(ui.next_event("lock"), json!({"locked": true}));
    assert!(!locking.is_finished(), "answered before the screens were covered");
    assert_eq!(ui.request(2, "report_locked", None)["result"], Value::Null);
    assert_eq!(locking.join().unwrap()["result"], Value::Null);
    assert!(novad.dir.join("locked").exists(), "kept for a restarted novad");

    // A UI that crashed while locked hears on its restart that it should lock.
    drop(ui);
    let mut restarted = novad.connect();
    restarted.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(restarted.next_event("lock"), json!({"locked": true}));
    // The screens stayed covered, so locking again answers at once.
    assert_eq!(novad.connect().request(1, "lock", None)["result"], Value::Null);

    assert_eq!(restarted.request(2, "report_unlocked", None)["result"], Value::Null);
    assert_eq!(restarted.next_event("lock"), json!({"locked": false}));
    assert!(!novad.dir.join("locked").exists());
}

#[test]
fn only_the_supervised_ui_reports_the_lock_screen() {
    let scripts = nova_test_support::TempDir::new("lock-ui");
    let fake_ui = write_program(&scripts, "quickshell", "exec sleep 30");
    let settings = UiSettings {
        quickshell: fake_ui,
        ui_dir: PathBuf::from("ui"),
        socket: PathBuf::from("unused.sock"),
        path_var: std::env::var("PATH").unwrap_or_default(),
        wayland_socket: None,
        egl_vendor: None,
        render_loop: None,
        icon_theme: "hicolor".into(),
    };
    let novad = TestDaemon::start("lock-reports", Some(settings));
    let mut other = novad.connect();
    for report in ["report_locked", "report_unlocked"] {
        let reply = other.request(1, report, None);
        assert!(reply["error"].as_str().unwrap().contains("only the UI"), "{report}: {reply}");
    }
    novad.daemon.ui.as_ref().unwrap().stop();
}

#[test]
fn lock_fails_when_the_screens_arent_covered_in_time() {
    let novad = TestDaemon::start("lock-stalled", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    // A UI that hears the lock but never covers the screens.
    let reply = novad.connect().request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("weren't covered"), "{reply}");
}

#[test]
fn lock_needs_its_feature_but_unlocking_doesnt() {
    let novad = TestDaemon::open("lock-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"lock\"]\n").unwrap()
    });
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    let reply = novad.connect().request(1, "lock", None);
    assert!(reply["error"].as_str().unwrap().contains("lock feature isn't ready"), "{reply}");
    // Turned off while the screen was locked: the UI can still say it unlocked.
    assert_eq!(ui.request(2, "report_unlocked", None)["result"], Value::Null);
}

#[test]
fn power_options_list_every_action_say_why_one_cant_run_and_when_the_system_booted() {
    let novad = TestDaemon::start("power", None);
    let menu = novad.connect().request(1, "power_options", None)["result"].clone();
    assert!(menu["booted_at"].as_u64().is_some_and(|booted| booted > 0), "the uptime's start: {menu}");
    let options = menu["options"].clone();
    let actions: Vec<&str> =
        options.as_array().unwrap().iter().map(|option| option["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["lock", "suspend", "log_out", "reboot", "power_off"]);
    let lock = &options[0];
    assert_eq!(lock["available"], true, "the lock feature is on by default: {lock}");
    // No compositor here, so nothing to quit.
    let log_out = &options[2];
    assert_eq!(log_out["available"], false);
    assert!(log_out["reason"].as_str().unwrap().contains("Not on this compositor"), "{log_out}");
    for option in options.as_array().unwrap() {
        assert!(option["available"] == true || option["reason"].is_string(), "a dimmed action says why: {option}");
    }
}

#[test]
fn power_needs_its_feature_and_logs_out_through_the_compositor() {
    let novad = TestDaemon::open("power-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"power\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    for (id, request) in [(1, json!(null)), (2, json!({"action": "lock"}))] {
        let command = if request.is_null() { "power_options" } else { "power" };
        let args = (!request.is_null()).then_some(request);
        let reply = cli.request(id, command, args);
        assert!(reply["error"].as_str().unwrap().contains("power feature isn't ready"), "{reply}");
    }

    let novad = TestDaemon::start("power-log-out", None);
    let reply = novad.connect().request(1, "power", Some(json!({"action": "log_out"})));
    assert!(reply["error"].as_str().unwrap().contains("doesn't know this compositor"), "{reply}");
}

#[test]
fn power_lock_locks_like_nova_lock() {
    let novad = TestDaemon::start("power-lock", None);
    let mut ui = novad.connect();
    ui.request(1, "subscribe", Some(json!({"topics": ["lock"]})));
    assert_eq!(ui.next_event("lock"), json!({"locked": false}));
    // The menu asks over the UI's own connection, which reports the screens
    // covered only after this answers: it doesn't wait for that.
    let asked = Instant::now();
    assert_eq!(ui.request(2, "power", Some(json!({"action": "lock"})))["result"], Value::Null);
    assert!(asked.elapsed() < Duration::from_secs(1), "waited {:?}", asked.elapsed());
    assert_eq!(ui.next_event("lock"), json!({"locked": true}));
    assert_eq!(ui.request(3, "report_locked", None)["result"], Value::Null);
    // Covered now, so `nova lock` answers at once.
    assert_eq!(novad.connect().request(1, "lock", None)["result"], Value::Null);
}
