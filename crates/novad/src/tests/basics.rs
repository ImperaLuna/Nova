//! The socket protocol, logs, status and the supervised UI.

use super::*;

#[test]
fn only_hello_arrives_until_subscribed() {
    let novad = TestDaemon::start("hello", None);
    let mut client = novad.connect();
    assert_eq!(client.request(1, "ping", None)["result"], "pong");
}

#[test]
fn subscribing_to_features_sends_their_state() {
    let novad = TestDaemon::start("features", None);
    let mut client = novad.connect();
    let reply = client.request(1, "subscribe", Some(json!({"topics": ["features"]})));
    assert_eq!(reply["result"], Value::Null);
    let event = client.next();
    assert_eq!(event["event"], "features");
    let features = event["data"]["features"].as_array().unwrap();
    let bar = features.iter().find(|feature| feature["id"] == "bar").unwrap();
    assert_eq!(bar["status"], "ready");
    assert!(
        bar["slots"].as_array().unwrap().contains(&json!({"host": "bar.center", "view": "central-pill", "order": 10}))
    );
}

#[test]
fn subscribing_to_the_theme_sends_its_colors() {
    let novad = TestDaemon::start("theme", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    let event = client.next();
    assert_eq!(event["event"], "theme");
    assert_eq!(event["data"]["id"], theme::DEFAULT_THEME_ID);
    assert_eq!(event["data"]["mode"], "dark");
    assert_eq!(event["data"]["colors"]["primary"], "#FD7014");
}

#[test]
fn requests_without_id_get_no_reply() {
    let novad = TestDaemon::start("no-id", None);
    let mut client = novad.connect();
    client.send(&json!({"v": 2, "cmd": "ping"}));
    assert_eq!(client.request(2, "ping", None)["result"], "pong");
}

#[test]
fn bad_requests_get_an_error_when_they_have_an_id() {
    let novad = TestDaemon::start("bad", None);
    let mut client = novad.connect();
    client.send_raw("not json");
    client.send(&json!({"v": 2, "id": 4, "cmd": "reboot"}));
    let reply = client.next();
    assert_eq!(reply["id"], 4);
    assert!(reply["error"].as_str().unwrap().starts_with("bad request"));
}

#[test]
fn unknown_fields_and_other_versions_are_refused() {
    let novad = TestDaemon::start("strict", None);
    let mut client = novad.connect();
    client.send(&json!({"v": 2, "id": 1, "cmd": "ping", "extra": true}));
    assert!(client.next()["error"].as_str().unwrap().contains("unknown field"));
    client.send(&json!({"v": 1, "id": 2, "cmd": "ping"}));
    assert!(client.next()["error"].as_str().unwrap().contains("version 1"));
    assert_eq!(client.request(3, "ping", None)["result"], "pong");
}

#[test]
fn oversized_requests_close_the_connection() {
    let novad = TestDaemon::start("oversized", None);
    let mut client = novad.connect();
    let huge = "x".repeat(usize::try_from(server::MAX_REQUEST_BYTES).unwrap() + 10);
    // novad may hang up before it has read the whole line, and a write it
    // stops reading mustn't hang the test.
    client.writer.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = writeln!(client.writer, "{huge}");
    // Hanging up on a line it hasn't read all of resets the connection
    // rather than ending it (a Unix socket closed with unread data), and
    // which one comes depends on how far novad read: both are a hang-up.
    client.writer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    match client.lines.next() {
        None => {}
        Some(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("novad should hang up, got {other:?}"),
    }
}

#[test]
fn a_client_that_stops_reading_is_disconnected() {
    let novad = TestDaemon::start("slow-client", None);
    let mut stalled = novad.connect();
    stalled.request(1, "subscribe", Some(json!({"topics": ["logs"]})));
    // Far more entries than the socket buffer and the queue hold together.
    for entry in 0..crate::hub::MAX_QUEUED_MESSAGES * 8 {
        novad.daemon.logger.info("test", &format!("entry {entry}"));
    }
    let delivered = stalled.lines.by_ref().map_while(Result::ok).count();
    assert!(delivered < crate::hub::MAX_QUEUED_MESSAGES * 8, "the stalled client was never cut off");
    let mut other = novad.connect();
    assert_eq!(other.request(1, "ping", None)["result"], "pong");
}

#[test]
fn a_client_that_stalls_with_its_socket_full_is_disconnected() {
    let novad = TestDaemon::start("stalled-client", None);
    let mut stalled = novad.connect();
    stalled.request(1, "subscribe", Some(json!({"topics": ["logs"]})));
    // Fills the socket buffer many times over but not the queue, so only the
    // write deadline can cut the client off.
    let entries = crate::hub::MAX_QUEUED_MESSAGES / 2;
    let padding = "x".repeat(1024);
    for entry in 0..entries {
        novad.daemon.logger.info("test", &format!("entry {entry} {padding}"));
    }
    std::thread::sleep(server::WRITE_TIMEOUT * 3);
    // Without the deadline everything arrives and the connection stays open.
    stalled.writer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let delivered = stalled.lines.by_ref().map_while(Result::ok).count();
    assert!(delivered < entries, "the stalled client was never cut off");
    let mut other = novad.connect();
    assert_eq!(other.request(1, "ping", None)["result"], "pong");
}

#[test]
fn a_second_novad_on_the_same_socket_is_refused() {
    let novad = TestDaemon::start("twice", None);
    let error = server::bind(&novad.socket).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
}

#[test]
fn log_subscribers_get_a_backlog_then_live_entries() {
    let novad = TestDaemon::start("logs", None);
    novad.daemon.logger.info("test", "before");
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["logs"]})));

    let backlog = client.next();
    assert_eq!(backlog["data"]["backlog"], true);
    let messages: Vec<&str> =
        backlog["data"]["entries"].as_array().unwrap().iter().map(|e| e["message"].as_str().unwrap()).collect();
    assert!(messages.contains(&"before"), "{messages:?}");

    novad.daemon.logger.warn("test", "after");
    // Features starting in the background (NetworkManager without a bus)
    // log their own entries meanwhile, so only this test's entry counts.
    let entry = loop {
        let live = client.next();
        assert_eq!(live["data"].get("backlog"), None);
        let entries = live["data"]["entries"].as_array().unwrap();
        if let Some(entry) = entries.iter().find(|entry| entry["source"] == "test") {
            break entry.clone();
        }
    };
    assert_eq!(entry["message"], "after");
    assert_eq!(entry["level"], "warn");
}

#[test]
fn status_reports_the_daemon_ui_and_features() {
    let novad = TestDaemon::start("status", None);
    let mut client = novad.connect();
    let status: Status = serde_json::from_value(client.request(1, "status", None)["result"].clone()).unwrap();
    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.ui.state, UiState::Disabled);
    assert!(status.features.iter().any(|feature| feature.id == "bar" && feature.status == FeatureStatus::Ready));
    assert!(status.pss_kib.is_some());
}

#[test]
fn a_crashing_ui_is_logged_snapshotted_and_restarted() {
    let scripts = nova_test_support::TempDir::new("ui");
    let fake_ui = write_program(&scripts, "quickshell", "echo '  WARN qml: about to fail'\nexit 3");

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
    let novad = TestDaemon::start("crash", Some(settings));
    let mut client = novad.connect();
    let status = wait_for_status(&mut client, |status| status.ui.restarts >= 1);

    let crash_log = PathBuf::from(&status.ui.last_crash.unwrap().log);
    let crash_entries = read_entries(&crash_log);
    let ui_line = crash_entries.iter().find(|entry| entry.source == UI_LOG_SOURCE).unwrap();
    assert_eq!((ui_line.level, ui_line.message.as_str()), (LogLevel::Warn, "qml: about to fail"));
    assert!(crash_entries.iter().any(|entry| entry.message.contains("exit code 3")));
    let full_log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    assert!(full_log.iter().any(|entry| entry.message.contains("failed to load")));
}

#[test]
fn a_quickshell_outside_the_tested_range_is_warned_about() {
    let scripts = nova_test_support::TempDir::new("old-quickshell");
    let fake_ui = scripts.join("quickshell");
    let script = "case \"$1\" in --version) echo 'Quickshell 0.1.0 (revision test)' ;; *) exec sleep 60 ;; esac";
    write_program(&scripts, "quickshell", script);

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
    let novad = TestDaemon::start("quickshell-range", Some(settings));
    let status = wait_for_status(&mut novad.connect(), |status| status.ui.pid.is_some());
    assert_eq!(status.quickshell.as_deref(), Some("0.1.0"));
    let log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    let warning = log.iter().find(|entry| entry.level == LogLevel::Warn && entry.message.contains("0.1.0"));
    assert!(warning.is_some_and(|entry| entry.message.contains("older than Nova is tested with")), "{log:?}");
    novad.daemon.ui.as_ref().unwrap().stop();
}

/// With every feature off, novad runs without Quickshell; turning on one
/// that draws (the bar) starts it, and turning it off again stops it without
/// counting a crash.
#[test]
fn the_ui_runs_only_while_a_feature_with_a_ui_is_on() {
    let scripts = nova_test_support::TempDir::new("ui-on-demand");
    let fake_ui = write_program(&scripts, "quickshell", "exec sleep 60");
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
    let ids: Vec<String> = nova_features::load_all(&repo_features()).unwrap().into_iter().map(|m| m.id).collect();
    let selection = |on: &[&str]| {
        let off: Vec<String> = ids.iter().filter(|id| !on.contains(&id.as_str())).map(|id| format!("{id:?}")).collect();
        format!("disabled = [{}]\n", off.join(", "))
    };
    let all_off = selection(&[]);
    let novad = TestDaemon::open("ui-on-demand", Some(settings), |dir| {
        std::fs::write(dir.join("config/features.toml"), &all_off).unwrap();
    });
    let mut client = novad.connect();
    let status: Status = serde_json::from_value(client.request(1, "status", None)["result"].clone()).unwrap();
    assert_eq!((status.ui.state, status.ui.pid), (UiState::NotNeeded, None));
    assert!(status.features.iter().all(|feature| feature.status == FeatureStatus::Off));

    std::fs::write(novad.features_file(), selection(&["bar"])).unwrap();
    wait_for_status(&mut client, |status| status.ui.state == UiState::Running);

    std::fs::write(novad.features_file(), &all_off).unwrap();
    let status = wait_for_status(&mut client, |status| status.ui.state == UiState::NotNeeded);
    assert_eq!((status.ui.pid, status.ui.restarts, status.ui.last_crash), (None, 0, None));

    // Off and straight back on, before the stopped UI has exited: a restart,
    // not a crash.
    let ui = novad.daemon.ui.as_ref().unwrap();
    ui.want(true, &novad.daemon.logger);
    let first = wait_for_status(&mut client, |status| status.ui.pid.is_some()).ui.pid;
    ui.want(false, &novad.daemon.logger);
    ui.want(true, &novad.daemon.logger);
    let status = wait_for_status(&mut client, |status| status.ui.pid.is_some_and(|pid| Some(pid) != first));
    assert_eq!((status.ui.restarts, status.ui.last_crash), (0, None));
    ui.stop();
}

fn wait_for_status(client: &mut Client, done: impl Fn(&Status) -> bool) -> Status {
    for attempt in 0..100 {
        let status: Status = serde_json::from_value(client.request(attempt, "status", None)["result"].clone()).unwrap();
        if done(&status) {
            return status;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("status never got there");
}
