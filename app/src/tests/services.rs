//! Screenshots, recording, host, tray, notifications, media, resources, peripherals and folder views over the socket.

use super::*;

#[test]
fn screenshots_need_their_feature_but_cancelling_doesnt() {
    let novad = TestDaemon::open("screenshot-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"screenshot\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    for (id, mode) in [(1, "region"), (2, "screen")] {
        let reply = cli.request(id, "screenshot", Some(json!({"mode": mode})));
        assert!(reply["error"].as_str().unwrap().contains("screenshot feature isn't ready"), "{reply}");
    }
    let rect = json!({"output": "DP-1", "rect": {"x": 0, "y": 0, "width": 1, "height": 1}});
    assert!(cli.request(3, "save_capture", Some(rect))["error"].is_string());
    assert_eq!(cli.request(4, "cancel_capture", None)["result"], Value::Null);
}

#[test]
fn recording_needs_its_feature_but_stopping_doesnt() {
    let novad = TestDaemon::open("recording-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"recording\"]\n").unwrap()
    });
    let mut cli = novad.connect();
    let reply = cli.request(1, "record", None);
    assert!(reply["error"].as_str().unwrap().contains("recording feature isn't ready"), "{reply}");
    let reply = cli.request(2, "start_recording", Some(json!({"output": "DP-1", "region": null})));
    assert!(reply["error"].as_str().unwrap().contains("recording feature isn't ready"), "{reply}");
    let reply = cli.request(3, "stop_recording", None);
    assert!(reply["error"].as_str().unwrap().contains("nothing is being recorded"), "{reply}");
    cli.request(4, "subscribe", Some(json!({"topics": ["recording"]})));
    assert_eq!(cli.next_event("recording"), Value::Null, "nothing is being recorded");
}

#[test]
fn subscribing_to_host_sends_the_machine_and_its_user() {
    let novad = TestDaemon::start("host", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["host"]})));
    let host = client.next_event("host");
    assert!(host["booted_at"].as_i64().is_some_and(|booted| booted > 0));
    assert!(host["user"].as_str().is_some_and(|user| !user.is_empty()));
    assert!(host["os"]["id"].is_string());

    std::fs::write(novad.config_file(), "[quick-settings]\nname = \"Katov\"\n").unwrap();
    assert_eq!(client.next_event("host")["user"], "Katov", "the config's name applies live");
}

/// The tray's own tests cover its bus; here it has none, so the list is empty.
#[test]
fn subscribing_to_the_tray_sends_its_items() {
    let novad = TestDaemon::start("tray", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["tray"]})));
    assert_eq!(client.next_event("tray"), json!({"items": []}));
}

/// The notifications' own tests cover the bus; here there's none, so the
/// list starts empty, and do not disturb and mutes work over the socket.
#[test]
fn notifications_are_listed_and_set_over_the_socket() {
    let novad = TestDaemon::start("notifications", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["notifications"]})));
    let empty = json!({"items": [], "do_not_disturb": false, "muted": [], "apps": []});
    assert_eq!(client.next_event("notifications"), empty);
    client.request(2, "set_do_not_disturb", Some(json!({"on": true})));
    assert_eq!(client.next_event("notifications")["do_not_disturb"], true);
    let refused = client.request(3, "mute_notification_app", Some(json!({"app": "Discord", "muted": true})));
    assert!(refused["error"].as_str().unwrap().contains("Discord"), "{refused}");
    let missing = client.request(4, "notification_action", Some(json!({"id": 9, "action": "reply"})));
    assert!(missing["error"].as_str().unwrap().contains("no notification 9"), "{missing}");
    let missing = client.request(5, "open_notification", Some(json!({"id": 9})));
    assert!(missing["error"].as_str().unwrap().contains("no notification 9"), "{missing}");
}

/// The media's own tests cover the bus; here there's none, so there are no
/// players, and commands for one are refused.
#[test]
fn media_players_are_listed_and_checked_over_the_socket() {
    let novad = TestDaemon::start("media", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["media"]})));
    assert_eq!(client.next_event("media"), json!({"players": []}));
    let player = "org.mpris.MediaPlayer2.mpv";
    let missing = client.request(2, "media_show", Some(json!({"player": player})));
    assert!(missing["error"].as_str().unwrap().contains("no player"), "{missing}");
    let missing = client.request(3, "media_control", Some(json!({"player": player, "action": "next"})));
    assert!(missing["error"].as_str().unwrap().contains("no player"), "{missing}");
    let long = "x".repeat(nova_media_api::MEDIA_PLAYER_MAX_BYTES + 1);
    let refused = client.request(4, "media_window", Some(json!({"player": long})));
    assert!(refused["error"].as_str().unwrap().contains("at most"), "{refused}");
    let extra = client.request(5, "media_seek", Some(json!({"player": player, "position_us": 1, "extra": 1})));
    assert!(extra["error"].as_str().unwrap().contains("unknown field"), "{extra}");
}

/// Resources come only while a connection watches, and stop once the last
/// one stops watching or hangs up.
#[test]
fn resources_are_sent_while_watched() {
    let novad = TestDaemon::start("resources", None);
    let mut listener = novad.connect();
    listener.request(1, "subscribe", Some(json!({"topics": ["resources"]})));
    let mut watcher = novad.connect();
    assert_eq!(watcher.request(1, "watch_resources", Some(json!({"watching": true})))["result"], Value::Null);
    let reading = listener.next_event("resources");
    assert!(reading["memory_total"].as_u64().is_some_and(|total| total > 0));
    assert!(reading["apps"].as_array().is_some_and(|apps| !apps.is_empty()));
    let nova = reading["apps"].as_array().unwrap().iter().find(|app| app["key"] == "nova");
    assert!(nova.is_some_and(|nova| nova["name"] == "Nova" && nova["protected"] == true), "Nova is listed: {reading}");
    assert!(reading["history"].is_array());

    drop(watcher);
    // Looks already sent when it hung up arrive first; then nothing.
    std::thread::sleep(nova_resources::LIVE_INTERVAL);
    listener.writer.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    while listener.lines.next().is_some_and(|line| line.is_ok()) {}
    listener.writer.set_read_timeout(Some(nova_resources::LIVE_INTERVAL * 2)).unwrap();
    let late = listener.lines.next();
    assert!(late.as_ref().is_none_or(Result::is_err), "still sent after the watcher left: {late:?}");
}

/// Ending an app only ends what Nova offers to: not Nova itself, and not
/// what isn't running.
#[test]
fn ending_nova_or_a_missing_app_is_refused() {
    let novad = TestDaemon::start("end-app", None);
    let mut client = novad.connect();
    let reply = client.request(1, "end_app", Some(json!({"key": "nova"})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("can't be ended")), "{reply}");
    let reply = client.request(2, "end_app", Some(json!({"key": "/nowhere/app"})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("no app")), "{reply}");
}

#[test]
fn watching_resources_needs_the_feature() {
    let novad = TestDaemon::open("resources-off", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "disabled = [\"ai-usage\", \"resources\"]\n").unwrap();
    });
    let mut client = novad.connect();
    let reply = client.request(1, "watch_resources", Some(json!({"watching": true})));
    assert!(reply["error"].as_str().is_some_and(|error| error.contains("the resources feature")), "{reply}");
}

/// The peripherals' own tests cover UPower and HeadsetControl; here there's
/// neither, so subscribing sends an empty list.
#[test]
fn subscribing_to_peripherals_sends_the_devices() {
    let novad = TestDaemon::start("peripherals", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["peripherals"]})));
    assert_eq!(client.next_event("peripherals"), json!({"devices": []}));
}
