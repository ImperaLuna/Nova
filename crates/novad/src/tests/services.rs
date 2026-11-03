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
    let long = "x".repeat(nova_protocol::MEDIA_PLAYER_MAX_BYTES + 1);
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
    std::thread::sleep(resources::LIVE_INTERVAL);
    listener.writer.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    while listener.lines.next().is_some_and(|line| line.is_ok()) {}
    listener.writer.set_read_timeout(Some(resources::LIVE_INTERVAL * 2)).unwrap();
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

/// Nova Files' views over the socket: a folder comes in a page, then its
/// changes, until the view closes. Its own tests cover paging and order.
#[test]
fn a_folder_view_is_listed_and_followed_over_the_socket() {
    let novad = TestDaemon::open("files", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "enabled = [\"files\"]\n").unwrap()
    });
    let folder = nova_test_support::TempDir::new("files-socket-folder");
    std::fs::write(folder.join("notes.txt"), b"hi").unwrap();
    let mut client = novad.connect();
    let sort = json!({"key": "name", "descending": false});
    let open = json!({"view": 1, "path": folder.to_str().unwrap(), "sort": sort, "hidden": false});
    assert_eq!(client.request(1, "files_open", Some(open))["result"], Value::Null);
    let page = client.next_event("files_page");
    assert_eq!((page["view"].clone(), page["start"].clone(), page["total"].clone()), (json!(1), json!(0), json!(1)));
    assert_eq!(page["entries"][0]["name"], "notes.txt");
    assert_eq!(page["entries"][0]["size"], 2);

    std::fs::create_dir(folder.join("Photos")).unwrap();
    let changed = client.next_event("files_changed");
    assert_eq!(changed["changes"][0]["op"], "insert");
    assert_eq!(changed["changes"][0]["index"], 0, "folders come first");
    assert_eq!(changed["changes"][0]["entry"]["dir"], true);

    assert_eq!(client.request(2, "files_close", Some(json!({"view": 1})))["result"], Value::Null);
    let relative = json!({"view": 2, "path": "Documents", "sort": sort, "hidden": false});
    assert!(client.request(3, "files_open", Some(relative))["error"].as_str().unwrap().contains("absolute"));
    let missing = json!({"view": 3, "path": folder.join("missing").to_str().unwrap(), "sort": sort, "hidden": false});
    assert_eq!(client.request(4, "files_open", Some(missing))["result"], Value::Null);
    let closed = client.next_event("files_closed");
    assert_eq!(closed["view"], 3);
    assert!(closed["error"].as_str().unwrap().contains("doesn't exist"), "{closed}");
}

/// Thumbnails over the socket: asked for, answered as an event. Their own
/// tests cover the cache, the thumbnailers and the queue.
#[test]
fn a_thumbnail_is_asked_for_and_answered_over_the_socket() {
    let novad = TestDaemon::open("files-thumbnail", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "enabled = [\"files\"]\n").unwrap()
    });
    let folder = nova_test_support::TempDir::new("files-socket-thumbnail");
    std::fs::write(folder.join("notes.txt"), b"hi").unwrap();
    let notes = folder.join("notes.txt").display().to_string();
    let mut client = novad.connect();
    assert_eq!(client.request(1, "files_thumbnail", Some(json!({"path": notes})))["result"], Value::Null);
    let answer = client.next_event("files_thumbnail");
    assert_eq!(answer["path"], json!(notes));
    assert!(answer["modified_ms"].is_i64(), "{answer}");
    assert!(answer.get("thumbnail").is_none(), "no thumbnailer takes text");
    let relative = json!({"path": "notes.txt"});
    assert!(client.request(2, "files_thumbnail", Some(relative))["error"].as_str().unwrap().contains("absolute"));
    let cancel = json!({"path": notes});
    assert_eq!(client.request(3, "files_thumbnail_cancel", Some(cancel))["result"], Value::Null);
}

/// Searching over the socket: names under Home, hidden folders included.
/// The search's own tests cover ranking and following changes.
#[test]
fn files_are_found_by_name_over_the_socket() {
    let novad = TestDaemon::open("files-search", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "enabled = [\"files\"]\n").unwrap();
        std::fs::create_dir_all(dir.join("home/.local/share/Steam/Crimson Desert")).unwrap();
    });
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["files"]})));
    let first = client.request(2, "files_search", Some(json!({"query": "crimson"})))["result"].clone();
    if first["ready"] == false {
        client.next_event("files_search_ready");
    }
    let mut id = 2;
    let mut search = |query: &str| {
        id += 1;
        client.request(id, "files_search", Some(json!({"query": query})))["result"].clone()
    };
    let found = search("crimson");
    assert_eq!(found["ready"], true, "built by the time files_search_ready comes");
    assert_eq!(found["total"], 1);
    assert_eq!(found["results"][0]["entry"]["name"], "Crimson Desert");
    assert_eq!(found["results"][0]["in_hidden_folder"], true);
    assert_eq!(found["results"][0]["marks"], json!([{"start": 0, "length": 7}]));

    let steam = novad.dir.join("home/.local/share/Steam");
    let within = |folder: &Path| json!({"query": "crimson", "within": folder.to_str().unwrap()});
    let inside = client.request(100, "files_search", Some(within(&steam)))["result"].clone();
    assert_eq!(inside["total"], 1);
    assert_eq!(inside["results"][0]["in_hidden_folder"], false, "hidden is counted below the folder searched");
    let below_it = client.request(101, "files_search", Some(within(&steam.join("Crimson Desert"))))["result"].clone();
    assert_eq!(below_it["total"], 0);
}

/// The Games place over the socket: in the places once Steam is there, and
/// its game listed. The Steam reader's own tests cover its files.
#[test]
fn installed_games_are_a_place_and_a_list_over_the_socket() {
    let novad = TestDaemon::open("files-games", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "enabled = [\"files\"]\n").unwrap();
        let manifest =
            "\"AppState\" { \"appid\" \"3321460\" \"name\" \"Crimson Desert\" \"installdir\" \"Crimson Desert\" }";
        std::fs::create_dir_all(dir.join("steam/steamapps/common/Crimson Desert")).unwrap();
        std::fs::write(dir.join("steam/steamapps/appmanifest_3321460.acf"), manifest).unwrap();
    });
    let mut client = novad.connect();
    let places = client.request(1, "files_places", None)["result"].clone();
    let kinds: Vec<_> = places.as_array().unwrap().iter().map(|place| place["kind"].clone()).collect();
    assert_eq!(kinds[kinds.len() - 2..], [json!("games"), json!("trash")]);
    let games = client.request(2, "files_games_list", None)["result"].clone();
    assert_eq!(games[0]["name"], "Crimson Desert");
    assert!(games[0]["folder"].as_str().unwrap().ends_with("steam/steamapps/common/Crimson Desert"));
    assert!(games[0].get("size").is_none(), "no size when Steam doesn't say");
}

/// The trash over the socket: an item trashed, listed, restored, and the
/// trash emptied, with `files_trash_changed` each time it changes. The
/// trash's own tests cover the spec.
#[test]
fn the_trash_is_used_and_followed_over_the_socket() {
    let novad = TestDaemon::open("files-trash", None, |dir| {
        std::fs::write(dir.join("config/features.toml"), "enabled = [\"files\"]\n").unwrap()
    });
    let folder = nova_test_support::TempDir::new("files-socket-trash");
    let notes = folder.join("notes.txt");
    std::fs::write(&notes, b"hi").unwrap();
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["files"]})));
    assert_eq!(client.request(2, "files_trash_list", None)["result"], json!([]));

    let paths = json!({"paths": [notes.to_str().unwrap()]});
    assert_eq!(client.request(3, "files_trash", Some(paths))["result"], Value::Null);
    client.next_event("files_trash_changed");
    let items = client.request(4, "files_trash_list", None)["result"].clone();
    assert_eq!(items[0]["entry"]["name"], "notes.txt");
    assert_eq!(items[0]["folder"], folder.to_str().unwrap());
    assert!(!notes.exists());

    let ids = json!({"ids": [items[0]["id"].clone()]});
    assert_eq!(client.request(5, "files_restore", Some(ids))["result"], Value::Null);
    assert!(notes.exists());
    let paths = json!({"paths": [notes.to_str().unwrap()]});
    assert_eq!(client.request(6, "files_trash", Some(paths))["result"], Value::Null);
    assert_eq!(client.request(7, "files_empty_trash", None)["result"], Value::Null);
    assert_eq!(client.request(8, "files_trash_list", None)["result"], json!([]));
    let missing = json!({"paths": [notes.to_str().unwrap()]});
    assert!(client.request(9, "files_delete", Some(missing))["error"].as_str().unwrap().contains("doesn't exist"));
}

#[test]
fn folder_views_need_their_feature() {
    let novad = TestDaemon::start("files-off", None);
    let mut client = novad.connect();
    let open = json!({"view": 1, "path": "/", "sort": {"key": "name", "descending": false}, "hidden": false});
    let reply = client.request(1, "files_open", Some(open));
    assert!(reply["error"].as_str().unwrap().contains("files feature isn't ready"), "{reply}");
}
