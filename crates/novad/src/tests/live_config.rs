//! Config and feature files applied live, and the folders they live in.

use super::usage::{CLAUDE_ENDPOINT_ON, claude_sign_in, fake_endpoint, provider};
use super::*;

/// Saves `text` to `path` the way many editors do: a new file renamed over
/// the old one.
fn save_by_rename(path: &Path, text: &str) {
    let partial = path.with_extension("toml.new");
    std::fs::write(&partial, text).unwrap();
    std::fs::rename(partial, path).unwrap();
}

fn feature_status<'a>(features: &'a Value, id: &str) -> &'a Value {
    &features["features"].as_array().unwrap().iter().find(|feature| feature["id"] == id).unwrap()["status"]
}

#[test]
fn changing_the_theme_mode_recolors_live() {
    let novad = TestDaemon::start("reload-theme", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    let dark = client.next_event("theme");
    assert_eq!(dark["mode"], "dark");

    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    let light = client.next_event("theme");
    assert_eq!(light["mode"], "light");
    assert_ne!(light["colors"], dark["colors"]);

    save_by_rename(&novad.config_file(), "[theme]\nmode = \"dark\"\n");
    assert_eq!(client.next_event("theme"), dark);
}

#[test]
fn a_broken_config_keeps_the_last_good_one_and_says_why() {
    let novad = TestDaemon::start("reload-broken", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme", "logs"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");

    save_by_rename(&novad.config_file(), "[theme]\nmode = \"sepia\"\n");
    loop {
        let message = client.next();
        assert_ne!(message["event"], "theme", "a broken file changed the theme");
        // TOML's error spans several lines, each its own entry.
        let entries = message["data"]["entries"].as_array().cloned().unwrap_or_default();
        let error = entries
            .iter()
            .filter(|entry| entry["level"] == "error")
            .map(|entry| entry["message"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        if error.contains("sepia") && error.contains("keeping the last good ones") {
            break;
        }
    }
    let mut other = novad.connect();
    other.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(other.next_event("theme")["mode"], "light");

    std::fs::write(novad.config_file(), "[theme]\nmode = \"dark\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "dark");
}

#[test]
fn broken_files_at_start_mean_defaults() {
    let novad = TestDaemon::open("start-broken", None, |dir| {
        std::fs::write(dir.join("config/config.toml"), "[theme\n").unwrap();
        std::fs::write(dir.join("config/features.toml"), "enabled = 3\n").unwrap();
    });
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme", "features"]})));
    assert_eq!(client.next_event("theme")["id"], theme::DEFAULT_THEME_ID);
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "ready");
    let log = read_entries(&nova_paths::log_file(&novad.dir.join("logs")));
    let errors = log.iter().filter(|entry| {
        entry.level == LogLevel::Error && entry.message.starts_with("broken settings, using the defaults: ")
    });
    assert_eq!(errors.count(), 2, "{log:?}");
}

#[test]
fn turning_a_feature_off_and_on_stops_and_starts_it() {
    let novad = TestDaemon::with_ai_usage("reload-feature", |_| {});
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["features", "ai_usage"]})));
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "ready");
    client.next_usage();

    save_by_rename(&novad.features_file(), WATCHERS_OFF);
    assert_eq!(feature_status(&client.next_event("features"), ai_usage::FEATURE_ID), "off");
    let reply = client.request(2, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(reply["error"], "the ai-usage feature isn't ready (see `nova status`)");

    // No features.toml means each feature's default, which is on.
    std::fs::remove_file(novad.features_file()).unwrap();
    // The restarted feature's worker publishes on its own thread, so either
    // event can come first.
    let (mut features, mut usage) = (None, None);
    while features.is_none() || usage.is_none() {
        let message = client.next();
        match message["event"].as_str() {
            Some("features") => features = Some(message["data"].clone()),
            Some("ai_usage") => usage = Some(message["data"].clone()),
            _ => {}
        }
    }
    assert_eq!(feature_status(&features.unwrap(), ai_usage::FEATURE_ID), "ready");
    assert_eq!(usage.unwrap()["shown"], "claude", "the restarted feature publishes again");
    let reply = client.request(3, "show_ai_usage", Some(json!({"provider": "codex"})));
    assert_eq!(reply["result"], Value::Null);
}

#[test]
fn turning_claudes_endpoint_on_applies_without_a_restart() {
    let (url, requests) = fake_endpoint(r#"{"five_hour":{"utilization":8.4,"resets_at":null}}"#);
    let novad = TestDaemon::with_ai_usage("reload-claude-endpoint", |root| claude_sign_in(root, &url));
    let mut client = novad.connect();
    assert_eq!(provider(&client.subscribe_usage(), "claude")["state"], "unavailable");
    assert!(requests.lock().unwrap().is_empty());

    std::fs::write(novad.config_file(), CLAUDE_ENDPOINT_ON).unwrap();
    while provider(&client.next_usage(), "claude")["state"] != "ready" {}
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn a_config_folder_made_after_start_is_watched() {
    let novad = TestDaemon::open("reload-new-folder", None, |dir| std::fs::remove_dir(dir.join("config")).unwrap());
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::create_dir(novad.config_file().parent().unwrap()).unwrap();
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
}

#[test]
fn edits_through_a_symlinked_config_are_seen() {
    let novad = TestDaemon::open("reload-symlink", None, |dir| {
        std::fs::create_dir(dir.join("dotfiles")).unwrap();
        std::fs::write(dir.join("dotfiles/config.toml"), "[theme]\nmode = \"dark\"\n").unwrap();
        std::os::unix::fs::symlink(dir.join("dotfiles/config.toml"), dir.join("config/config.toml")).unwrap();
    });
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    std::fs::write(novad.dir.join("dotfiles/config.toml"), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
}

#[test]
fn a_replaced_config_folder_is_followed() {
    let novad = TestDaemon::start("reload-replaced-folder", None);
    let mut client = novad.connect();
    client.request(1, "subscribe", Some(json!({"topics": ["theme"]})));
    assert_eq!(client.next_event("theme")["mode"], "dark");
    let folder = novad.config_file().parent().unwrap().to_owned();
    std::fs::rename(&folder, novad.dir.join("config.old")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(novad.features_file(), WATCHERS_OFF).unwrap();
    std::fs::write(novad.config_file(), "[theme]\nmode = \"light\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "light");
    // Edits in the new folder keep applying.
    std::fs::write(novad.config_file(), "[theme]\nmode = \"dark\"\n").unwrap();
    assert_eq!(client.next_event("theme")["mode"], "dark");
}
